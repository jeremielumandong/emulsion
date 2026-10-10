//! PSD blending metadata must never silently change meaning or apply twice.
use super::*;
use emulsion_raster::composite::{BlendIf, BlendIfChannel};

#[test]
fn channel_specific_blend_if_metadata_stays_exact_but_renderer_eligibility_is_unproven() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["psd", "psb"] {
        for channel in [
            BlendIfChannel::Red,
            BlendIfChannel::Green,
            BlendIfChannel::Blue,
        ] {
            let mut doc = Document::new(8, 6);
            doc.blend_space = emulsion_raster::blend::BlendSpace::Srgb;
            add(
                &mut doc,
                Node::raster(
                    0,
                    "Background",
                    Arc::new(Raster::from_srgba8(8, 6, &[51, 179, 77, 255].repeat(8 * 6))),
                    Placement::default(),
                ),
                None,
            )
            .unwrap();
            let id = add(
                &mut doc,
                Node::raster(
                    0,
                    "Color gate",
                    Arc::new(Raster::from_srgba8(8, 6, &[179, 51, 3, 255].repeat(8 * 6))),
                    Placement::default(),
                ),
                None,
            )
            .unwrap();
            doc.node_mut(id).unwrap().blending.blend_if = BlendIf {
                channel,
                source: BlendRange {
                    black: 80.0 / 255.0,
                    black_fade: 200.0 / 255.0,
                    white_fade: 1.0,
                    white: 1.0,
                },
                ..Default::default()
            };
            let path = dir.path().join(format!("channel.{ext}"));
            assert!(needs_appearance_fallback(&doc));
            let report = write_with_report(&doc, &path).unwrap();
            assert_eq!(
                report.appearance_fallback,
                Some(AppearanceFallback::UnsupportedFeatures)
            );
            let back = read(&path).unwrap();
            assert_eq!(back.nodes.len(), 1);
            assert_eq!(
                flatten(&back.composite_tree(), 0).to_srgba8(),
                flatten(&doc.composite_tree(), 0).to_srgba8()
            );
            // Encoder-only evidence is separate from renderer eligibility.
            let psd = Psd {
                width: 8.0,
                height: 6.0,
                children: Some(
                    doc.children(None)
                        .into_iter()
                        .map(|id| layer_for(&doc, doc.node(id).unwrap()))
                        .collect(),
                ),
                ..Default::default()
            };
            let raw = ag_psd::write_psd(
                &psd,
                &WriteOptions {
                    no_background: Some(true),
                    ..Default::default()
                },
            );
            let parsed = ag_psd::read_psd(&raw, &ReadOptions::default()).unwrap();
            let metadata_only = from_psd(&parsed).unwrap();
            // Independent third-party PSD fixture establishes a 40-byte RGB(A)
            // table: Gray, R, G, B, neutral fourth channel. Pin actual written
            // bytes rather than merely trusting our importer to agree.
            let mut packet = 40u32.to_be_bytes().to_vec();
            packet.extend_from_slice(&[0, 0, 255, 255, 0, 0, 255, 255].repeat(5));
            let color_index = match channel {
                BlendIfChannel::Red => 1,
                BlendIfChannel::Green => 2,
                BlendIfChannel::Blue => 3,
                BlendIfChannel::Gray => unreachable!(),
            };
            let offset = 4 + color_index * 8;
            packet[offset..offset + 8].copy_from_slice(&[80, 200, 255, 255, 0, 0, 255, 255]);
            assert_eq!(
                raw.windows(packet.len())
                    .filter(|bytes| *bytes == packet)
                    .count(),
                1,
                "one exact 40-byte channel-specific range table"
            );
            assert_eq!(
                metadata_only
                    .nodes
                    .iter()
                    .find(|n| n.name == "Color gate")
                    .unwrap()
                    .blending
                    .blend_if,
                doc.node(id).unwrap().blending.blend_if
            );
        }
    }
}

#[test]
fn deep_knockout_uses_named_appearance_instead_of_becoming_shallow() {
    let mut doc = Document::new(8, 8);
    add(
        &mut doc,
        Node::raster(
            0,
            "Backdrop",
            Arc::new(Raster::solid(8, 8, [0.1, 0.2, 0.8, 1.0])),
            Placement::default(),
        ),
        None,
    )
    .unwrap();
    let group = add(&mut doc, Node::group(0, "Isolated group"), None).unwrap();
    doc.node_mut(group).unwrap().blend = BlendMode::Normal;
    add(
        &mut doc,
        Node::raster(
            0,
            "Group base",
            Arc::new(Raster::solid(8, 8, [0.8, 0.2, 0.1, 1.0])),
            Placement::default(),
        ),
        Some(group),
    )
    .unwrap();
    let hole = add(
        &mut doc,
        Node::raster(
            0,
            "Deep hole",
            Arc::new(Raster::solid(4, 4, [0.2, 0.8, 0.1, 1.0])),
            Placement::at(2.0, 2.0),
        ),
        Some(group),
    )
    .unwrap();
    doc.node_mut(hole).unwrap().blending.knockout = Knockout::Deep;
    doc.node_mut(hole).unwrap().blending.fill_opacity = 0.0;
    let before = doc.clone();
    assert!(needs_appearance_fallback(&doc));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep.psd");
    write(&doc, &path).unwrap();
    let back = read(&path).unwrap();
    assert_eq!(back.nodes.len(), 1);
    assert!(back.nodes[0].name.contains("appearance"));
    assert_eq!(
        flatten(&back.composite_tree(), 0).to_srgba8(),
        flatten(&doc.composite_tree(), 0).to_srgba8()
    );
    assert_eq!(doc, before);
}

#[test]
fn rasterized_content_does_not_bake_and_reapply_fill_or_channel_gates() {
    let mut doc = Document::new(32, 32);
    let id = add(
        &mut doc,
        Node::raster(
            0,
            "Transformed Fill",
            Arc::new(Raster::solid(16, 16, [0.7, 0.2, 0.1, 1.0])),
            Placement::at(4.5, 4.5),
        ),
        None,
    )
    .unwrap();
    doc.node_mut(id).unwrap().blending.fill_opacity = 128.0 / 255.0;
    let node = doc.node(id).unwrap();
    let layer = layer_for(&doc, node);
    let data = layer.image_data.as_ref().unwrap();
    assert_eq!(
        data.data.as_chunks::<4>().0.iter().map(|p| p[3]).max(),
        Some(255),
        "Fill belongs only to iOpa; layer pixels retain full alpha"
    );
    assert_eq!(
        layer.additional_info.fill_opacity,
        Some(f64::from(128.0f32 / 255.0))
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fill.psd");
    write(&doc, &path).unwrap();
    let back = read(&path).unwrap();
    let a = flatten(&doc.composite_tree(), 0).to_srgba8();
    let b = flatten(&back.composite_tree(), 0).to_srgba8();
    assert!(
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1),
        "8-bit rasterized edge/Fill quantization is at most one byte"
    );
    doc.node_mut(id).unwrap().blending.blend_if = BlendIf {
        channel: BlendIfChannel::Red,
        source: BlendRange {
            black: 0.5,
            black_fade: 1.0,
            white_fade: 1.0,
            white: 1.0,
        },
        ..Default::default()
    };
    let gated = layer_for(&doc, doc.node(id).unwrap());
    assert_eq!(
        gated.image_data.unwrap().data,
        data.data,
        "Blend If also belongs only to metadata"
    );
}

#[test]
fn blend_space_difference_is_reported_by_the_actual_export_job() {
    let dir = tempfile::tempdir().unwrap();
    for mode in [BlendMode::Multiply, BlendMode::Screen, BlendMode::Overlay] {
        let mut doc = Document::new(4, 4);
        add(
            &mut doc,
            Node::raster(
                0,
                "Backdrop",
                Arc::new(Raster::from_srgba8(4, 4, &[118, 179, 218, 255].repeat(16))),
                Placement::default(),
            ),
            None,
        )
        .unwrap();
        let id = add(
            &mut doc,
            Node::raster(
                0,
                "Blend",
                Arc::new(Raster::from_srgba8(4, 4, &[218, 149, 89, 255].repeat(16))),
                Placement::default(),
            ),
            None,
        )
        .unwrap();
        doc.node_mut(id).unwrap().blend = mode;
        assert!(
            !needs_appearance_fallback(&doc),
            "structural gate does not guess pixel differences"
        );
        let expected = flatten(&doc.composite_tree(), 0).to_srgba8();
        let path = dir.path().join("space.psd");
        let report = crate::export::export_with_workflow_report(
            &doc,
            &path,
            crate::ExportOptions::for_doc(&doc),
            crate::export::ExportWorkflow::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            report.appearance_fallback,
            Some(AppearanceFallback::BlendSpaceDifference)
        );
        let back = read(&path).unwrap();
        assert_eq!(back.nodes.len(), 1);
        assert_eq!(flatten(&back.composite_tree(), 0).to_srgba8(), expected);
        doc.blend_space = emulsion_raster::blend::BlendSpace::Srgb;
        let report = write_with_report(&doc, &path).unwrap();
        assert_eq!(report.appearance_fallback, None);
        assert_eq!(read(&path).unwrap().nodes.len(), 2);
    }
    // Identity-looking Multiply remains layered; merely selecting Linear is
    // not enough to force fallback or discard newly editable masks.
    let mut doc = Document::new(4, 4);
    let id = add(
        &mut doc,
        Node::raster(
            0,
            "Only layer",
            Arc::new(Raster::solid(4, 4, [0.7, 0.2, 0.1, 1.0])),
            Placement::default(),
        ),
        None,
    )
    .unwrap();
    doc.node_mut(id).unwrap().blend = BlendMode::Multiply;
    let report = write_with_report(&doc, &dir.path().join("same.psb")).unwrap();
    assert_eq!(report.appearance_fallback, None);
}

#[test]
fn clipped_group_uses_appearance_without_changing_native_group_behavior() {
    let mut doc = Document::new(12, 8);
    let base = add(
        &mut doc,
        Node::raster(
            0,
            "Clip base",
            Arc::new(Raster::solid(6, 8, [0.2, 0.3, 0.8, 1.0])),
            Placement::default(),
        ),
        None,
    )
    .unwrap();
    let group = add(&mut doc, Node::group(0, "Clipped group"), None).unwrap();
    add(
        &mut doc,
        Node::raster(
            0,
            "Group child",
            Arc::new(Raster::solid(12, 8, [0.8, 0.1, 0.2, 1.0])),
            Placement::default(),
        ),
        Some(group),
    )
    .unwrap();
    Command::SetClip {
        id: group,
        clip_to: Some(base),
    }
    .apply(&mut doc)
    .unwrap();
    assert!(needs_appearance_fallback(&doc));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clipped-group.psd");
    let before = doc.clone();
    let report = write_with_report(&doc, &path).unwrap();
    assert_eq!(
        report.appearance_fallback,
        Some(AppearanceFallback::UnsupportedFeatures)
    );
    let back = read(&path).unwrap();
    assert_eq!(back.nodes.len(), 1);
    assert_eq!(
        flatten(&back.composite_tree(), 0).to_srgba8(),
        flatten(&doc.composite_tree(), 0).to_srgba8()
    );
    assert_eq!(doc, before);
}

#[test]
fn opaque_native_bottom_layer_keeps_explicit_transparency_channel() {
    let mut doc = Document::new(4, 3);
    add(
        &mut doc,
        Node::raster(
            0,
            "Ordinary opaque layer",
            Arc::new(Raster::solid(4, 3, [0.2, 0.3, 0.4, 1.0])),
            Placement::default(),
        ),
        None,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    for ext in ["psd", "psb"] {
        let path = dir.path().join(format!("ordinary.{ext}"));
        write(&doc, &path).unwrap();
        let bytes = std::fs::read(path).unwrap();
        let mut at = 26usize;
        for _ in 0..2 {
            let n = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            at += 4 + n;
        }
        let wide = if ext == "psb" { 8 } else { 4 };
        at += wide * 2; // Layer/mask and nested layer-info lengths.
        at += 2 + 16; // Count and first (bottom) rectangle.
        let count = u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
        at += 2;
        let channels: Vec<i16> = (0..count)
            .map(|_| {
                let id = i16::from_be_bytes(bytes[at..at + 2].try_into().unwrap());
                at += 2 + wide;
                id
            })
            .collect();
        assert!(
            channels.contains(&-1),
            "an ordinary bottom layer must not become PSD Background"
        );
    }
}
