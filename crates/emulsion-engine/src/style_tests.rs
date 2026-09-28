//! Styled layers must retain the compositor's semantics and scalable text.
use crate::{Camera, Engine, Gpu, Offscreen, Output, vector::VectorSpace};
use emulsion_core::{Command, Document, Node, command::Slot, styles::LayerStyle};
use emulsion_raster::{BlendMode, Placement, Raster, blend::BlendSpace, color::px_to_f};
use std::sync::Arc;

fn gpu() -> Option<Arc<Gpu>> {
    match Gpu::new(crate::gpu::instance(), None, None) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            assert!(
                std::env::var("EMULSION_REQUIRE_GPU_TESTS").as_deref() != Ok("1"),
                "GPU tests required: {error:#}"
            );
            eprintln!("Skipping GPU checks: {error:#}");
            None
        }
    }
}

fn add(doc: &mut Document, node: Node) -> emulsion_core::NodeId {
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(doc)
    .unwrap()
    .unwrap()
}

fn shadow(distance: f32) -> LayerStyle {
    LayerStyle::DropShadow {
        color: [0, 0, 0],
        opacity: 65.,
        angle: 135.,
        distance,
        size: 3.,
    }
}

fn render(engine: &mut Engine, zoom: f64) -> Vec<[f32; 4]> {
    let size = (
        (f64::from(engine.canvas.width) * zoom) as u32,
        (f64::from(engine.canvas.height) * zoom) as u32,
    );
    engine.screen = size;
    engine.camera = Camera {
        center: [
            f64::from(engine.canvas.width) / 2.,
            f64::from(engine.canvas.height) / 2.,
        ],
        zoom,
    };
    let output = Offscreen::new(&engine.gpu, size, wgpu::TextureFormat::Rgba32Float);
    engine
        .render(&output.view, output.format, Output::Raw)
        .unwrap();
    output
        .read(&engine.gpu)
        .unwrap()
        .as_chunks::<16>()
        .0
        .iter()
        .map(|bytes| {
            std::array::from_fn(|c| f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap()))
        })
        .collect()
}

fn assert_cpu(engine: &mut Engine, doc: &Document) {
    assert!(
        engine.canvas.unsupported.is_empty(),
        "{:?}",
        engine.canvas.unsupported
    );
    let actual = render(engine, 1.);
    let expected = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
    let mut worst = 0_f32;
    for (i, pixel) in actual.iter().enumerate() {
        let reference = px_to_f(expected.get(i as u32 % doc.width, i as u32 / doc.width));
        for c in 0..4 {
            worst = worst.max((pixel[c] - reference[c]).abs());
        }
    }
    assert!(
        worst < 0.002,
        "styled composite differs from CPU by {worst}"
    );
}

#[test]
fn styles_match_cpu_with_blends_clipping_opacity_and_cache() {
    let Some(gpu) = gpu() else { return };
    for space in [BlendSpace::Linear, BlendSpace::Srgb] {
        for effect_blend in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen] {
            for clipped in [false, true] {
                let mut doc = Document::new(64, 64);
                doc.blend_space = space;
                add(
                    &mut doc,
                    Node::raster(
                        0,
                        "Backdrop",
                        Arc::new(Raster::solid(64, 64, [0.15, 0.25, 0.35, 0.6])),
                        Placement::default(),
                    ),
                );
                let mut node = Node::raster(
                    0,
                    "Styled",
                    Arc::new(Raster::solid(20, 18, [0.4, 0.12, 0.06, 0.7])),
                    Placement::at(20., 18.),
                );
                node.opacity = 0.7;
                node.blend = BlendMode::Multiply;
                node.styles = vec![
                    shadow(7.),
                    LayerStyle::OuterGlow {
                        color: [180, 40, 90],
                        opacity: 45.,
                        size: 3.,
                    },
                    LayerStyle::Stroke {
                        color: [20, 200, 100],
                        opacity: 70.,
                        size: 2.,
                    },
                    LayerStyle::InnerShadow {
                        color: [0, 0, 0],
                        opacity: 55.,
                        angle: 135.,
                        distance: 3.,
                        size: 2.,
                    },
                    LayerStyle::ColorOverlay {
                        color: [20, 110, 200],
                        opacity: 30.,
                    },
                ];
                node.style_options = vec![
                    emulsion_core::style_options::StyleOptions {
                        blend: effect_blend,
                        ..Default::default()
                    };
                    node.styles.len()
                ];
                add(&mut doc, node);
                let styled = doc.nodes.iter().find(|n| n.name == "Styled").unwrap().id;
                if clipped {
                    let clipped = Node::raster(
                        0,
                        "Clipped",
                        Arc::new(Raster::solid(64, 64, [0.1, 0.3, 0.1, 0.5])),
                        Placement::default(),
                    );
                    let id = add(&mut doc, clipped);
                    Command::SetClip {
                        id,
                        clip_to: Some(styled),
                    }
                    .apply(&mut doc)
                    .unwrap();
                }
                for cache in [false, true] {
                    let mut engine = Engine::new(
                        gpu.clone(),
                        &doc,
                        Some(styled),
                        VectorSpace::Srgb,
                        false,
                        cache,
                        (64, 64),
                    )
                    .unwrap();
                    assert!(
                        engine.canvas.paint.is_none(),
                        "styled paint must rebuild effects"
                    );
                    assert_cpu(&mut engine, &doc);
                    let original = doc.clone();
                    doc.node_mut(styled).unwrap().styles[0] = shadow(12.);
                    engine.reload(&doc, Some(styled), false).unwrap();
                    assert_cpu(&mut engine, &doc);
                    engine.reload(&original, Some(styled), false).unwrap();
                    assert_cpu(&mut engine, &original);
                    doc = original;
                }
            }
        }
    }
}

#[test]
fn shadow_keeps_text_foreground_identical_at_fractional_and_high_zoom() {
    let Some(gpu) = gpu() else { return };
    for zoom in [1., 1.5, 2.5] {
        let mut doc = Document::new(320, 160);
        let spec = emulsion_core::text::TextSpec {
            text: "Sharp café".into(),
            font: "Geist".into(),
            size: 38.,
            x: 22.25,
            y: 25.5,
            rotation: 7.,
            color: [255; 4],
            ..Default::default()
        };
        add(&mut doc, Node::text(0, "Text", spec, 320, 160));
        let id = doc.nodes[0].id;
        let size = ((320. * zoom) as u32, (160. * zoom) as u32);
        let mut engine =
            Engine::new(gpu.clone(), &doc, None, VectorSpace::Srgb, true, true, size).unwrap();
        let plain = render(&mut engine, zoom);
        doc.node_mut(id).unwrap().styles = vec![shadow(8.)];
        engine.reload(&doc, None, true).unwrap();
        assert_eq!(engine.canvas.vector_count(), 1);
        assert!(engine.canvas.unsupported.is_empty());
        assert!(engine.canvas.rasterized.is_empty());
        let styled = render(&mut engine, zoom);
        let mut shadow_pixels = 0;
        for (a, b) in plain.iter().zip(&styled) {
            // A black shadow changes coverage, not the premultiplied RGB of
            // the white foreground. Every antialiased glyph edge must survive.
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() < 0.0001, "text softened at zoom {zoom}");
            }
            shadow_pixels += usize::from(b[3] > a[3] + 0.1);
        }
        assert!(
            shadow_pixels > 100,
            "the shadow must actually render: zoom={zoom}, pixels={shadow_pixels}, ops={:?}",
            engine.canvas.ops
        );
        let original = styled;
        Command::TranslateNode {
            id,
            dx: 3.25,
            dy: 2.5,
        }
        .apply(&mut doc)
        .unwrap();
        engine.reload(&doc, None, true).unwrap();
        assert_ne!(
            render(&mut engine, zoom),
            original,
            "editing styled text must invalidate both text and effects"
        );
        let clipped = Node::raster(
            0,
            "Clipped",
            Arc::new(Raster::solid(320, 160, [0.1, 0.2, 0.3, 0.5])),
            Placement::default(),
        );
        let clipped_id = add(&mut doc, clipped);
        Command::SetClip {
            id: clipped_id,
            clip_to: Some(id),
        }
        .apply(&mut doc)
        .unwrap();
        engine.reload(&doc, None, true).unwrap();
        assert_eq!(
            engine.canvas.vector_count(),
            2,
            "text and its clipping shape"
        );
        assert!(engine.vectors.edit(id, |kind| {
            let crate::canvas::VectorKind::Text { spec } = kind else {
                panic!("text")
            };
            Arc::make_mut(spec).x += 2.;
        }));
        let instances: Vec<_> = engine
            .vectors
            .objects
            .iter()
            .filter(|object| object.node == id)
            .collect();
        assert_eq!(instances.len(), 2);
        let specs: Vec<_> = instances
            .iter()
            .map(|object| {
                let crate::canvas::VectorKind::Text { spec } = &object.kind else {
                    panic!("text")
                };
                spec
            })
            .collect();
        assert_eq!(
            specs[0], specs[1],
            "incremental edits must update the clipping shape too"
        );
    }
}

#[test]
fn responsive_content_clip_is_analytic_at_zoom_and_keeps_native_text() {
    use emulsion_core::{
        NodeKind,
        design_layout::{Child, Frame},
    };
    use emulsion_raster::{vector::PathStyle, vector_geometry};
    let Some(gpu) = gpu() else { return };
    let mut doc = Document::new(96, 64);
    let group = add(&mut doc, Node::group(0, "Frame"));
    let mut append = |node: Node| {
        Command::AddNode {
            node: Box::new(node),
            slot: Slot::top_of(Some(group)),
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap()
    };
    let border = append(Node::path(
        0,
        "Border",
        Arc::new(vector_geometry::rectangle(20.25, 10., 40., 30.)),
        PathStyle {
            fill: None,
            stroke: None,
            ..Default::default()
        },
        96,
        64,
    ));
    let fill = append(Node::new(
        0,
        "Fill",
        NodeKind::Fill {
            rgba: [255, 0, 0, 255],
        },
    ));
    let mut overlay = Node::new(
        0,
        "Overlay",
        NodeKind::Fill {
            rgba: [0, 0, 255, 255],
        },
    );
    overlay.clip_to = Some(fill);
    let overlay = append(overlay);
    let text = append(Node::text(
        0,
        "Text",
        emulsion_core::text::TextSpec {
            text: "Sharp glyphs".into(),
            x: 5.,
            y: 15.,
            size: 18.,
            color: [0, 0, 0, 255],
            ..Default::default()
        },
        96,
        64,
    ));
    doc.design.frames.insert(
        group,
        Frame {
            boundary: border,
            clip_content: true,
            children: [fill, overlay, text]
                .into_iter()
                .map(|id| {
                    (
                        id,
                        Child {
                            absolute: true,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            ..Default::default()
        },
    );
    let mut engine = Engine::new(gpu, &doc, None, VectorSpace::Srgb, true, true, (96, 64)).unwrap();
    assert!(engine.canvas.rasterized.is_empty());
    for zoom in [1., 2., 4.] {
        let actual = render(&mut engine, zoom);
        let width = (96. * zoom) as usize;
        let row = (12. * zoom) as usize;
        for x in 0..width {
            let lo = (x as f64 / zoom).max(20.25);
            let hi = ((x + 1) as f64 / zoom).min(60.25);
            let expected = ((hi - lo) * zoom).clamp(0., 1.) as f32;
            assert!(
                (actual[row * width + x][3] - expected).abs() < 0.002,
                "zoom{zoom} x{x}: expected alpha{expected}, got{}",
                actual[row * width + x][3]
            );
        }
    }
    assert!(
        matches!(&doc.node(text).unwrap().kind,NodeKind::Text{cache,..} if !cache.is_rendered()),
        "content clipping cannot force glyphs through document-resolution rasterization"
    );
}
