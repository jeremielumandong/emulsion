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
    let mut fallback;
    let engine = if engine.canvas.unsupported.is_empty() {
        engine
    } else {
        assert!(
            engine
                .canvas
                .unsupported
                .iter()
                .all(|reason| reason.starts_with("grouped clipping ")),
            "{:?}",
            engine.canvas.unsupported
        );
        let mut flat = Document::new(doc.width, doc.height);
        flat.nodes.push(Node::raster(
            1,
            "CPU grouped-clipping fallback",
            Arc::new(emulsion_raster::composite::flatten(
                &doc.composite_tree(),
                0,
            )),
            Placement::default(),
        ));
        fallback = Engine::new(
            engine.gpu.clone(),
            &flat,
            None,
            VectorSpace::Srgb,
            false,
            false,
            (doc.width, doc.height),
        )
        .unwrap();
        &mut fallback
    };
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

// Keep mask/effect coverage on the real GPU when no clipping stack is active.
// Grouped variants assert the refusal contract and CPU texture presentation.
fn assert_mask_routes(engine: &mut Engine, doc: &Document, clip: u64, base: u64) {
    for grouped in [false, true] {
        let mut candidate = doc.clone();
        candidate.node_mut(clip).unwrap().clip_to = grouped.then_some(base);
        engine.reload(&candidate, None, false).unwrap();
        if grouped {
            assert!(
                engine
                    .canvas
                    .unsupported
                    .iter()
                    .any(|reason| reason.starts_with("grouped clipping "))
            );
        } else {
            assert!(
                engine.canvas.unsupported.is_empty(),
                "ungrouped masks must retain real GPU execution: {:?}",
                engine.canvas.unsupported
            );
        }
        assert_cpu(engine, &candidate);
    }
}

fn assert_styled_clip_uses_original_shape(doc: &Document, base: u64, clip: u64) {
    use emulsion_raster::composite::{CompositeTree, NodeContent, flatten};
    let tree = doc.composite_tree();
    let node = tree.nodes.iter().find(|node| node.id == base).unwrap();
    let NodeContent::StyledGroup { clip_source, .. } = &node.content else {
        panic!("styled base")
    };
    let shape = flatten(
        &CompositeTree {
            width: doc.width,
            height: doc.height,
            space: doc.blend_space,
            nodes: vec![clip_source.as_ref().clone()],
        },
        0,
    );
    let emulsion_core::NodeKind::Raster { raster, .. } = &doc.node(clip).unwrap().kind else {
        panic!("clipped raster")
    };
    let actual = flatten(&tree, 0);
    for y in 0..doc.height {
        for x in 0..doc.width {
            let coverage = px_to_f(shape.get(x, y))[3];
            let expected = px_to_f(raster.get(x, y)).map(|v| v * coverage);
            let pixel = px_to_f(actual.get(x, y));
            for (a, b) in pixel.into_iter().zip(expected) {
                assert!(
                    (a - b).abs() < 6e-5,
                    "clip shape at ({x},{y}): {pixel:?} != {expected:?}"
                );
            }
        }
    }
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

#[test]
#[ignore = "Requires a real host GPU; run serial with EMULSION_REQUIRE_GPU_TESTS=1"]
fn native_multistop_gradients_keep_vector_edges_at_zoom() {
    use emulsion_raster::vector::{GradientStop, PathPaint, PathStyle};
    let Some(gpu) = gpu() else {
        return;
    };
    let mut doc = Document::new(64, 48);
    let stops = [
        GradientStop {
            offset: 0.,
            color: [255, 0, 0, 255],
        },
        GradientStop {
            offset: 0.5,
            color: [0, 255, 0, 255],
        },
        GradientStop {
            offset: 1.,
            color: [0, 0, 255, 255],
        },
    ];
    let id = add(
        &mut doc,
        Node::path(
            0,
            "Native gradient",
            Arc::new(emulsion_raster::vector_geometry::rectangle(
                8.25, 8., 40., 30.,
            )),
            PathStyle {
                fill: Some(stops[0].color),
                fill_paint: PathPaint::from_stops(&stops, false, 0.).unwrap(),
                stroke: None,
                ..Default::default()
            },
            64,
            48,
        ),
    );
    let emulsion_core::NodeKind::Path { style, .. } = &doc.node(id).unwrap().kind else {
        panic!()
    };
    assert!(crate::canvas::path_supported(style));
    let mut engine = Engine::new(gpu, &doc, None, VectorSpace::Srgb, true, true, (64, 48)).unwrap();
    for zoom in [1., 2., 4.] {
        let output = render(&mut engine, zoom);
        let width = (64. * zoom) as usize;
        let y = (20. * zoom) as usize;
        let edge = (8.25 * zoom).floor() as usize;
        let expected = 1. - (8.25 * zoom).fract();
        assert!(
            (output[y * width + edge][3] - expected as f32).abs() < 0.04,
            "zoom {zoom} edge"
        );
        let center = output[y * width + (28. * zoom) as usize];
        assert!(
            center[1] > 0.9,
            "green middle stop remains native at zoom {zoom}: {center:?}"
        );
    }
}

#[test]
fn native_vector_masks_match_cpu_with_independent_components_styles_and_reload() {
    use emulsion_core::{MaskProperties, VectorMask};
    use emulsion_raster::{Mask, vector::Path};
    let Some(gpu) = gpu() else { return };
    for cache in [false, true] {
        for effect_blend in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen] {
            let mut doc = Document::new(48, 40);
            let background = add(
                &mut doc,
                Node::raster(
                    0,
                    "Background",
                    Arc::new(Raster::solid(48, 40, [0.1, 0.2, 0.3, 0.5])),
                    Placement::default(),
                ),
            );
            let mut node = Node::raster(
                0,
                "Two masks",
                Arc::new(Raster::solid(28, 24, [0.5, 0.1, 0.2, 0.75])),
                Placement::at(8., 7.),
            );
            node.opacity = 0.7;
            node.blend = BlendMode::Multiply;
            node.mask = Some(Arc::new(Mask::from_fn(28, 24, 255, |x, _| {
                if x < 10 { 70 } else { 255 }
            })));
            node.mask_properties = MaskProperties {
                density: 0.8,
                feather: 1.,
            };
            node.vector_mask = Some(VectorMask {
                path: Arc::new(Path::from_svg("M 2 2 C 18 -3 29 12 22 20 L 4 23 Z").unwrap()),
                properties: MaskProperties {
                    density: 0.9,
                    feather: 1.5,
                },
                transform: [1., 0.1, -0.1, 1., 1., -1.],
                ..Default::default()
            });
            node.styles = vec![
                shadow(3.),
                LayerStyle::ColorOverlay {
                    color: [180, 30, 200],
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
            let id = add(&mut doc, node);
            let clip = add(
                &mut doc,
                Node::raster(
                    0,
                    "Clipped",
                    Arc::new(Raster::solid(48, 40, [0.1, 0.35, 0.1, 0.5])),
                    Placement::default(),
                ),
            );
            Command::SetClip {
                id: clip,
                clip_to: Some(id),
            }
            .apply(&mut doc)
            .unwrap();
            let mut engine = Engine::new(
                gpu.clone(),
                &doc,
                None,
                VectorSpace::Srgb,
                false,
                cache,
                (48, 40),
            )
            .unwrap();
            for raster_enabled in [false, true] {
                for vector_enabled in [false, true] {
                    doc.node_mut(id).unwrap().mask_enabled = raster_enabled;
                    doc.node_mut(id)
                        .unwrap()
                        .vector_mask
                        .as_mut()
                        .unwrap()
                        .enabled = vector_enabled;
                    for hides in [false, true] {
                        doc.node_mut(id).unwrap().blending.layer_mask_hides_effects = hides;
                        engine.reload(&doc, None, false).unwrap();
                        assert_mask_routes(&mut engine, &doc, clip, id);

                        // Isolate the clipped sibling. Its coverage must be the
                        // original masked content, never the styled silhouette
                        // or the effect mask applied a second time.
                        let mut clipping_only = doc.clone();
                        clipping_only.node_mut(background).unwrap().visible = false;
                        clipping_only.node_mut(id).unwrap().opacity = 0.0;
                        engine.reload(&clipping_only, None, false).unwrap();
                        // Styled roots deliberately retain their existing
                        // compatibility behavior; compare to their independent
                        // original shape, not an unstyled root whose grouped
                        // opacity now correctly attenuates its entire stack.
                        assert_cpu(&mut engine, &clipping_only);
                        assert_styled_clip_uses_original_shape(&clipping_only, id, clip);
                    }
                }
            }
            Command::SetVectorMaskPath {
                id,
                path: Arc::new(Path::from_svg("M 8 4 L 25 5 L 18 22 Z").unwrap()),
            }
            .apply(&mut doc)
            .unwrap();
            engine.reload(&doc, None, false).unwrap();
            assert_mask_routes(&mut engine, &doc, clip, id);
        }
    }
}

#[test]
fn smart_filter_masks_match_cpu_with_stack_alpha_masks_clipping_effects_and_reload() {
    use emulsion_core::smart::{Filter, FilterStyle};
    use emulsion_core::{MaskProperties, SmartFilterMask, VectorMask};
    use emulsion_raster::{Mask, vector::Path};
    let Some(gpu) = gpu() else { return };
    for cache in [false, true] {
        let mut doc = Document::new(48, 40);
        add(
            &mut doc,
            Node::raster(
                0,
                "Backdrop",
                Arc::new(Raster::solid(48, 40, [0.1, 0.2, 0.3, 0.6])),
                Placement::default(),
            ),
        );
        let source = Arc::new(Raster::from_fn(19, 15, [0; 4], |x, y| {
            if (2..17).contains(&x) && (2..13).contains(&y) {
                [22000, 8000, 4000, 32000]
            } else {
                [0; 4]
            }
        }));
        let mut node = Node::smart(
            0,
            "Masked stack",
            source,
            vec![Filter::GaussianBlur { radius: 2. }],
            Placement::at(13., 11.),
        );
        node.mask = Some(Arc::new(Mask::from_fn(19, 15, 255, |x, _| {
            if x < 4 { 64 } else { 240 }
        })));
        node.mask_properties = MaskProperties {
            density: 0.8,
            feather: 1.,
        };
        node.vector_mask = Some(VectorMask {
            path: Arc::new(Path::from_svg("M 1 1 L 18 2 L 15 14 L 2 12 Z").unwrap()),
            ..Default::default()
        });
        node.styles = vec![
            shadow(3.),
            LayerStyle::ColorOverlay {
                color: [160, 40, 190],
                opacity: 30.,
            },
        ];
        node.opacity = 0.75;
        node.blend = BlendMode::Multiply;
        let id = add(&mut doc, node);
        Command::SetSmartFilterMask {
            id,
            mask: Some(SmartFilterMask::new(Arc::new(Mask::from_fn(
                19,
                15,
                255,
                |x, _| if x < 10 { 0 } else { 128 },
            )))),
        }
        .apply(&mut doc)
        .unwrap();
        let clip = add(
            &mut doc,
            Node::raster(
                0,
                "Clipped",
                Arc::new(Raster::solid(48, 40, [0.2, 0.3, 0.1, 0.5])),
                Placement::default(),
            ),
        );
        Command::SetClip {
            id: clip,
            clip_to: Some(id),
        }
        .apply(&mut doc)
        .unwrap();
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            None,
            VectorSpace::Srgb,
            false,
            cache,
            (48, 40),
        )
        .unwrap();
        assert_mask_routes(&mut engine, &doc, clip, id);
        for layer_enabled in [false, true] {
            for vector_enabled in [false, true] {
                for filter_enabled in [false, true] {
                    doc.node_mut(id).unwrap().mask_enabled = layer_enabled;
                    doc.node_mut(id)
                        .unwrap()
                        .vector_mask
                        .as_mut()
                        .unwrap()
                        .enabled = vector_enabled;
                    Command::SetSmartFilterMaskEnabled {
                        id,
                        enabled: filter_enabled,
                    }
                    .apply(&mut doc)
                    .unwrap();
                    for hides in [false, true] {
                        doc.node_mut(id).unwrap().blending.layer_mask_hides_effects = hides;
                        engine.reload(&doc, None, false).unwrap();
                        assert_mask_routes(&mut engine, &doc, clip, id);
                    }
                }
            }
        }
        for command in [
            Command::SetSmartFilterMaskPixels {
                id,
                pixels: Arc::new(Mask::from_fn(
                    19,
                    15,
                    255,
                    |x, y| if x < y { 0 } else { 128 },
                )),
            },
            Command::SetSmartFilterMaskProperties {
                id,
                properties: MaskProperties {
                    density: 0.6,
                    feather: 2.,
                },
            },
            Command::SetSmartFilterMaskTransform {
                id,
                transform: [1., 0.15, -0.1, 1., -1., 1.],
            },
            Command::SetFilterStack {
                id,
                filters: vec![Filter::GaussianBlur { radius: 3. }, Filter::FindEdges],
                styles: vec![
                    FilterStyle {
                        opacity: 0.7,
                        blend: BlendMode::Screen,
                    },
                    FilterStyle::default(),
                ],
            },
            Command::SetSmartFilterMask { id, mask: None },
        ] {
            command.apply(&mut doc).unwrap();
            engine.reload(&doc, None, false).unwrap();
            assert_mask_routes(&mut engine, &doc, clip, id);
        }
    }
}
