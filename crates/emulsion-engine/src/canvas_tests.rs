use super::*;
use emulsion_raster::{Mask, Placement, TILE_PX};

fn pixel_node(raster: Arc<Raster>, placement: Placement) -> CompositeNode {
    CompositeNode {
        id: 1,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        blending: Default::default(),
        mask: Some(Arc::new(Mask::from_fn(900, 700, 255, |x, _| {
            (x % 256) as u8
        }))),
        clip_to: None,
        clip_rect: None,
        content: NodeContent::Pixels {
            raster: raster.into(),
            placement,
        },
    }
}

fn cached(node: &CompositeNode, width: u32, height: u32) -> Baked {
    Baked {
        key: Compiler::bake_key(node, false),
        node: node.clone(),
        raster: bake(node, width, height, BlendSpace::Linear, None),
        source: 0,
    }
}

fn assert_pixels_equal(a: &Raster, b: &Raster) {
    let (nx, ny) = a.tiles_at(0);
    for y in 0..ny {
        for x in 0..nx {
            let c = TileCoord::new(x, y);
            assert_eq!(a.tile(0, c), b.tile(0, c), "tile {c:?}");
        }
    }
}

#[test]
fn styled_effect_masks_do_not_enable_other_advanced_blending() {
    use emulsion_raster::composite::{BlendIf, BlendRange, BlendingOptions, Knockout};

    let mut doc = Document::new(16, 16);
    let mut node = emulsion_core::Node::raster(
        1,
        "Styled",
        Arc::new(Raster::solid(16, 16, [0.2, 0.1, 0.3, 0.5])),
        Placement::default(),
    );
    node.mask = Some(Arc::new(Mask::empty(16, 16, 120)));
    node.styles
        .push(emulsion_core::styles::LayerStyle::ColorOverlay {
            color: [180, 30, 200],
            opacity: 30.,
        });
    let supported = BlendingOptions {
        layer_mask_hides_effects: true,
        ..Default::default()
    };
    node.blending = supported;
    doc.nodes.push(node);
    let warnings = |doc: &Document| {
        let mut compiler = Compiler {
            vectors: HashMap::new(),
            names: HashMap::from([(1, "Styled".into())]),
            width: doc.width,
            height: doc.height,
            space: doc.blend_space,
            sources: Vec::new(),
            ops: Vec::new(),
            runs: Vec::new(),
            open_run: None,
            unsupported: Vec::new(),
            rasterized: Vec::new(),
            alpha_slots: 0,
            paint: None,
            paint_node: None,
            baked_prev: Vec::new(),
            baked_new: Vec::new(),
            _doc: doc,
        };
        compiler.list(&doc.composite_tree().nodes, 0);
        compiler.unsupported
    };
    for enabled in [false, true] {
        doc.nodes[0].mask_enabled = enabled;
        assert!(warnings(&doc).is_empty());
    }
    for (name, blending) in [
        (
            "fill opacity",
            BlendingOptions {
                fill_opacity: 0.5,
                ..supported
            },
        ),
        (
            "channels",
            BlendingOptions {
                channels: [false, true, true],
                ..supported
            },
        ),
        (
            "BlendIf",
            BlendingOptions {
                blend_if: BlendIf {
                    source: BlendRange {
                        black: 0.1,
                        black_fade: 0.2,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..supported
            },
        ),
        (
            "knockout",
            BlendingOptions {
                knockout: Knockout::Shallow,
                ..supported
            },
        ),
        (
            "interior grouping",
            BlendingOptions {
                blend_interior_effects_as_group: false,
                ..supported
            },
        ),
        (
            "clipping grouping",
            BlendingOptions {
                blend_clipped_layers_as_group: false,
                ..supported
            },
        ),
        (
            "transparency shape",
            BlendingOptions {
                transparency_shapes_layer: false,
                ..supported
            },
        ),
    ] {
        doc.nodes[0].blending = blending;
        assert_eq!(
            warnings(&doc),
            ["Styled: advanced blending options ignored"],
            "{name} must retain its unsupported diagnostic"
        );
    }
}

#[test]
fn chart_labels_and_table_cells_remain_native_vector_text() {
    use emulsion_core::design_charts::{self, Chart, Kind};
    for kind in Kind::ALL {
        let mut editor = emulsion_core::Editor::new(Document::new(800, 600), None);
        design_charts::apply(&mut editor, None, Chart::example(kind), (30.5, 40.25)).unwrap();
        let vectors = vector_nodes(&editor.doc);
        let mut labels = 0;
        for node in &editor.doc.nodes {
            if let NodeKind::Text { spec, cache } = &node.kind {
                labels += 1;
                assert!(spec.width.is_some() && spec.height.is_some());
                assert!(
                    matches!(vectors.get(&node.id), Some(VectorKind::Text { .. })),
                    "{} / {} must not become a document-resolution bitmap",
                    kind.label(),
                    node.name
                );
                assert!(!cache.is_rendered());
            }
        }
        assert!(labels > 1);
    }
}

#[test]
fn changed_bakes_match_full_render_through_edit_erase_and_undo() {
    let base = Arc::new(Raster::solid(900, 700, [0.2, 0.1, 0.05, 0.5]));
    let edited = Arc::new(base.with_changes(vec![(
        TileCoord::new(1, 1),
        Some(vec![[30000, 0, 0, 40000]; TILE_PX]),
    )]));
    let erased =
        Arc::new(edited.with_changes(vec![(TileCoord::new(1, 1), Some(vec![[0; 4]; TILE_PX]))]));
    for placement in [
        Placement::default(),
        Placement::at(31.0, 67.0),
        Placement::at(-73.0, -19.0),
        Placement::at(0.5, 2.25),
        Placement {
            scale_x: 0.05,
            scale_y: 0.3,
            rotation: 31.0,
            ..Default::default()
        },
        Placement {
            scale_x: 1.3,
            flip_x: true,
            rotation: -25.0,
            ..Default::default()
        },
        Placement {
            scale_x: 0.5,
            scale_y: 0.5,
            rotation: 23.0,
            ..Default::default()
        },
    ] {
        let mut node = pixel_node(base.clone(), placement);
        let mut previous = cached(&node, 1000, 800);
        for raster in [&edited, &erased, &base, &edited] {
            node.content = NodeContent::Pixels {
                raster: raster.clone().into(),
                placement,
            };
            let result = bake(&node, 1000, 800, BlendSpace::Linear, Some(&previous));
            let reference = bake(&node, 1000, 800, BlendSpace::Linear, None);
            assert_pixels_equal(&result, &reference);
            if let Some(dirty) = changed_bake_tiles(&previous.node, &node, 1000, 800) {
                assert!(!dirty.is_empty() && dirty.len() <= 9);
                for (c, tile) in previous.raster.base_tiles() {
                    if !dirty.contains(c) {
                        assert!(
                            Arc::ptr_eq(tile, result.base_tile(*c).unwrap()),
                            "unchanged tile lost: {placement:?} {c:?}"
                        );
                    }
                }
            }
            previous = Baked {
                key: Compiler::bake_key(&node, false),
                raster: result,
                node: node.clone(),
                source: 0,
            };
        }
    }
}

#[test]
fn changed_mask_fill_size_and_placement_do_not_reuse_stale_bakes() {
    let base = Arc::new(Raster::solid(900, 700, [0.2, 0.1, 0.05, 0.5]));
    let original = pixel_node(base, Placement::default());
    let previous = cached(&original, 1000, 800);
    let mut cases = Vec::new();
    let mut node = original.clone();
    node.mask = Some(Arc::new(Mask::empty(900, 700, 127)));
    cases.push(node);
    let mut node = original.clone();
    node.content = NodeContent::Pixels {
        raster: Arc::new(Raster::solid(900, 700, [0.0, 0.2, 0.0, 0.5])).into(),
        placement: Placement::default(),
    };
    cases.push(node);
    let mut node = original.clone();
    node.content = NodeContent::Pixels {
        raster: Arc::new(Raster::solid(300, 250, [0.2, 0.1, 0.05, 0.5])).into(),
        placement: Placement::at(256.0, 0.0),
    };
    cases.push(node);
    for node in cases {
        for (w, h) in [(1000, 800), (1100, 900)] {
            assert_pixels_equal(
                &bake(&node, w, h, BlendSpace::Linear, Some(&previous)),
                &bake(&node, w, h, BlendSpace::Linear, None),
            );
        }
    }
}

#[test]
fn document_signatures_are_stable_without_rasterizing_vectors() {
    let mut doc = Document::new(900, 700);
    for node in [
        emulsion_core::Node::raster(
            0,
            "pixels",
            Arc::new(Raster::transparent(900, 700)),
            Placement::default(),
        ),
        emulsion_core::Node::text(
            0,
            "text",
            emulsion_core::text::TextSpec::default(),
            900,
            700,
        ),
    ] {
        emulsion_core::Command::AddNode {
            node: Box::new(node),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
    }
    let a = Canvas::signature(&doc);
    let b = Canvas::signature(&doc);
    assert!(Canvas::pixels_only_change(&a, &b).unwrap().is_empty());
    for node in doc.composite_tree().nodes {
        if let NodeContent::Pixels { raster, .. } = node.content
            && matches!(doc.node(node.id).unwrap().kind, NodeKind::Text { .. })
        {
            assert!(!raster.is_ready());
        }
    }
    let id = doc
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Text { .. }))
        .unwrap()
        .id;
    emulsion_core::Command::TranslateNode {
        id,
        dx: 1.0,
        dy: 2.0,
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(
        Canvas::pixels_only_change(&b, &Canvas::signature(&doc)),
        Some(vec![id])
    );
}

#[test]
#[ignore = "timing comparison; run in release mode with --nocapture"]
fn benchmark_masked_tile_rebake() {
    let base = Arc::new(Raster::solid(3840, 2160, [0.2, 0.1, 0.05, 0.5]));
    let mut original = pixel_node(base.clone(), Placement::default());
    original.mask = Some(Arc::new(Mask::empty(3840, 2160, 127)));
    let previous = cached(&original, 3840, 2160);
    let mut edited = original.clone();
    edited.content = NodeContent::Pixels {
        raster: Arc::new(base.with_changes(vec![(
            TileCoord::new(1, 1),
            Some(vec![[30000, 0, 0, 40000]; TILE_PX]),
        )]))
        .into(),
        placement: Placement::default(),
    };
    for (label, prior) in [("full", None), ("incremental", Some(&previous))] {
        let mut times = Vec::new();
        for _ in 0..7 {
            let start = std::time::Instant::now();
            std::hint::black_box(bake(&edited, 3840, 2160, BlendSpace::Linear, prior));
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "4K masked one-tile rebake {label}: median {:.3} ms",
            times[3]
        );
    }
}

#[test]
fn gpu_reload_preserves_tables_and_matches_pixels_after_baked_edits() {
    use crate::vector::VectorSpace;
    use crate::{Engine, Gpu, Offscreen, Output};
    let gpu = match Gpu::new(crate::gpu::instance(), None, None) {
        Ok(gpu) => gpu,
        Err(error) => {
            assert!(
                std::env::var("EMULSION_REQUIRE_GPU_TESTS").as_deref() != Ok("1"),
                "GPU required: {error:#}"
            );
            eprintln!("Skipping GPU check: {error:#}");
            return;
        }
    };
    eprintln!("GPU: {}", gpu.describe());
    for masked in [false, true] {
        for placement in [Placement::default(), Placement::at(17.0, -9.0)] {
            let mut doc = Document::new(512, 512);
            let original = Arc::new(Raster::solid(512, 512, [0.2, 0.1, 0.05, 0.5]));
            let edited = Arc::new(original.with_changes(vec![(
                TileCoord::new(0, 0),
                Some(vec![[30000, 0, 0, 40000]; TILE_PX]),
            )]));
            let mut node = emulsion_core::Node::raster(0, "pixels", original.clone(), placement);
            if masked {
                node.mask = Some(Arc::new(Mask::empty(512, 512, 127)));
            }
            let id = emulsion_core::Command::AddNode {
                node: Box::new(node),
                slot: emulsion_core::command::Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap()
            .unwrap();
            let mut engine = Engine::new(
                gpu.clone(),
                &doc,
                None,
                VectorSpace::Srgb,
                true,
                true,
                (512, 512),
            )
            .unwrap();
            let target = Offscreen::new(&gpu, (512, 512), wgpu::TextureFormat::Rgba32Float);
            engine.camera = crate::Camera {
                center: [256.0, 256.0],
                zoom: 1.0,
            };
            engine
                .render(&target.view, target.format, Output::Raw)
                .unwrap();
            let tables = engine.canvas.tables.clone();
            // Edit, undo, redo, then change an implicit fill. Each render starts
            // with a populated composite cache, so stale damage is visible.
            for raster in [
                &edited,
                &original,
                &edited,
                &Arc::new(Raster::solid(512, 512, [0.0, 0.2, 0.0, 0.5])),
            ] {
                if let NodeKind::Raster {
                    raster: current, ..
                } = &mut doc.node_mut(id).unwrap().kind
                {
                    *current = raster.clone();
                }
                engine.reload(&doc, None, true).unwrap();
                assert_eq!(
                    engine.canvas.tables, tables,
                    "pixel edit rebuilt tile tables"
                );
                engine
                    .render(&target.view, target.format, Output::Raw)
                    .unwrap();
                let bytes = target.read(&gpu).unwrap();
                let actual: &[f32] = bytemuck::cast_slice(&bytes);
                let reference = flatten(&doc.composite_tree(), 0);
                let mut max_error = 0.0_f32;
                for y in 0..512 {
                    for x in 0..512 {
                        let expected = emulsion_raster::color::px_to_f(reference.get(x, y));
                        let offset = ((y * 512 + x) * 4) as usize;
                        for c in 0..4 {
                            max_error = max_error.max((actual[offset + c] - expected[c]).abs());
                        }
                    }
                }
                assert!(
                    max_error < 0.001,
                    "masked={masked}, {placement:?}, error={max_error}, first={:?}, reference={:?}, slots={:?}",
                    &actual[..4],
                    reference.get(0, 0),
                    engine.canvas.sources[0].slots
                );
                engine.reload(&doc, None, true).unwrap();
                assert!(engine.canvas.dirty.is_empty());
            }
            // Structural edits must reuse the last refreshed bake and remain
            // valid after the fast path has updated its inputs.
            let refreshed = engine.canvas.sources[0].raster.clone();
            doc.node_mut(id).unwrap().opacity = 0.75;
            engine.reload(&doc, None, true).unwrap();
            assert!(engine.canvas.unsupported.is_empty());
            assert!(Arc::ptr_eq(&refreshed, &engine.canvas.sources[0].raster));
        }
    }
}

#[test]
fn responsive_clips_keep_native_text_and_invalidate_canvas_signature() {
    use emulsion_core::{
        Command, Node,
        command::Slot,
        design_layout::{Child, Frame},
    };
    use emulsion_raster::{vector::PathStyle, vector_geometry};
    let mut doc = Document::new(96, 64);
    let mut add = |node: Node, parent| {
        Command::AddNode {
            node: Box::new(node),
            slot: Slot::top_of(parent),
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap()
    };
    let group = add(Node::group(0, "Frame"), None);
    let border = add(
        Node::path(
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
        ),
        Some(group),
    );
    let text = add(
        Node::text(
            0,
            "Text",
            emulsion_core::text::TextSpec {
                text: "Sharp glyphs".into(),
                x: 5.,
                y: 10.,
                size: 20.,
                ..Default::default()
            },
            96,
            64,
        ),
        Some(group),
    );
    doc.design.frames.insert(
        group,
        Frame {
            boundary: border,
            clip_content: true,
            children: std::collections::BTreeMap::from([(
                text,
                Child {
                    absolute: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        },
    );
    let before = Canvas::signature(&doc);
    let mut compiler = Compiler {
        vectors: vector_nodes(&doc),
        names: HashMap::new(),
        width: 96,
        height: 64,
        space: doc.blend_space,
        sources: Vec::new(),
        ops: Vec::new(),
        runs: Vec::new(),
        open_run: None,
        unsupported: Vec::new(),
        rasterized: Vec::new(),
        alpha_slots: 0,
        paint: None,
        paint_node: None,
        baked_prev: Vec::new(),
        baked_new: Vec::new(),
        _doc: &doc,
    };
    compiler.list(&doc.composite_tree().nodes, 0);
    assert!(compiler.rasterized.is_empty());
    assert!(compiler.runs.iter().flatten().any(|item| item.node == text));
    assert!(compiler.ops.iter().any(|op| matches!(
        op,
        Op::ClipPop {
            clip_rect: Some([20.25, 10., 40., 30.]),
            ..
        }
    )));
    assert!(
        matches!(&doc.node(text).unwrap().kind,NodeKind::Text{cache,..} if !cache.is_rendered())
    );
    doc.design.frames.get_mut(&group).unwrap().clip_content = false;
    assert!(
        Canvas::pixels_only_change(&before, &Canvas::signature(&doc)).is_none(),
        "clip changes rebuild GPU program instead of reusing stale pixels"
    );
}

#[test]
fn ordinary_diagram_groups_share_one_vector_target() {
    use emulsion_core::diagram::{Builder, ShapeKind};
    let mut b = Builder::new(1200, 900).unwrap();
    for i in 0..1000 {
        b.add_shape(
            ShapeKind::Process,
            [
                10. + (i % 40) as f64 * 25.,
                10. + (i / 40) as f64 * 30.,
                20.,
                20.,
            ],
            "n",
        )
        .unwrap();
    }
    let doc = b.finish().unwrap();
    let compile = |doc: &Document| {
        let mut c = Compiler {
            vectors: vector_nodes(doc),
            names: HashMap::new(),
            width: doc.width,
            height: doc.height,
            space: doc.blend_space,
            sources: Vec::new(),
            ops: Vec::new(),
            runs: Vec::new(),
            open_run: None,
            unsupported: Vec::new(),
            rasterized: Vec::new(),
            alpha_slots: 0,
            paint: None,
            paint_node: None,
            baked_prev: Vec::new(),
            baked_new: Vec::new(),
            _doc: doc,
        };
        c.list(&doc.composite_tree().nodes, 0);
        (c.runs.len(), c.ops)
    };
    let (runs, ops) = compile(&doc);
    assert_eq!(runs, 1);
    assert!(!ops.iter().any(|o| matches!(o, Op::Push { .. })));
    let mut translucent = doc.clone();
    let group = *translucent
        .diagram
        .as_ref()
        .unwrap()
        .shapes
        .keys()
        .next()
        .unwrap();
    translucent.node_mut(group).unwrap().opacity = 0.5;
    let (runs, ops) = compile(&translucent);
    assert!(runs > 1);
    assert!(ops.iter().any(|o| matches!(o, Op::Push { .. })));
    translucent.node_mut(group).unwrap().blend = BlendMode::Normal;
    let (_, ops) = compile(&translucent);
    assert!(ops.iter().any(|o| matches!(o, Op::Push { isolated: true })));
}

#[test]
fn bitmap_diagram_assets_keep_vector_labels_gpu_eligible() {
    use emulsion_core::{
        Node,
        diagram::{Builder, ShapeKind},
    };
    let mut b = Builder::new(800, 600).unwrap();
    b.add_shape(
        ShapeKind::Process,
        [40., 40., 120., 60.],
        "Native vector text",
    )
    .unwrap();
    let mut doc = b.finish().unwrap();
    assert!(diagram_vector_supported(&doc));
    let id = doc.alloc_id();
    doc.nodes.push(Node::raster(
        id,
        "Original bitmap",
        Arc::new(Raster::solid(32, 32, [0.2, 0.3, 0.4, 1.])),
        Placement::default(),
    ));
    assert!(diagram_gpu_supported(&doc));
    assert!(
        !diagram_vector_supported(&doc),
        "Mixed diagrams must not enter vector-only updates"
    );
    doc.node_mut(id).unwrap().mask = Some(Arc::new(Mask::from_fn(32, 32, 255, |_, _| 128)));
    assert!(
        !diagram_gpu_supported(&doc),
        "Unsupported appearance must retain fallback"
    );
}

#[test]
fn gpu_brush_on_upper_layer_stays_stable_across_frames_and_commit() {
    use crate::brush::{GpuStroke, test_brush};
    use crate::vector::VectorSpace;
    use crate::{Engine, Gpu, Offscreen, Output};
    let gpu = match Gpu::new(crate::gpu::instance(), None, None) {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("Skipping GPU check: {error:#}");
            return;
        }
    };
    let size = 512u32;
    for (shared_top, zoom) in [(false, 1.0), (true, 1.0), (true, 0.5), (true, 0.25)] {
        let mut doc = Document::new(size, size);
        let mid = Arc::new(Raster::solid(size, size, [0.0, 0.0, 0.3, 0.4]));
        let top_raster = if shared_top {
            mid.clone()
        } else {
            Arc::new(Raster::transparent(size, size))
        };
        let mut ids = Vec::new();
        for (name, raster) in [
            (
                "bg",
                Arc::new(Raster::solid(size, size, [0.9, 0.9, 0.9, 1.0])),
            ),
            ("mid", mid.clone()),
            ("top", top_raster),
        ] {
            ids.push(
                emulsion_core::Command::AddNode {
                    node: Box::new(emulsion_core::Node::raster(
                        0,
                        name,
                        raster,
                        Placement::default(),
                    )),
                    slot: emulsion_core::command::Slot::TOP,
                }
                .apply(&mut doc)
                .unwrap()
                .unwrap(),
            );
        }
        let top = ids[2];
        let screen = (size as f64 * zoom).ceil() as u32;
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            None,
            VectorSpace::Srgb,
            true,
            true,
            (screen, screen),
        )
        .unwrap();
        engine.camera = crate::Camera {
            center: [size as f64 / 2.0, size as f64 / 2.0],
            zoom,
        };
        let level = engine.camera.level();
        let target = Offscreen::new(&gpu, (screen, screen), wgpu::TextureFormat::Rgba32Float);
        // Compare against the engine's own uncached render of the same state:
        // any difference is a stale cache or atlas tile, i.e. visible flicker.
        let check = |engine: &mut Engine, doc: &Document, what: &str| {
            engine
                .render(&target.view, target.format, Output::Raw)
                .unwrap();
            let bytes = target.read(&gpu).unwrap();
            let actual: Vec<f32> = bytemuck::cast_slice(&bytes).to_vec();
            let mut fresh = Engine::new(
                gpu.clone(),
                doc,
                None,
                VectorSpace::Srgb,
                false,
                false,
                (screen, screen),
            )
            .unwrap();
            fresh.camera = engine.camera;
            fresh
                .render(&target.view, target.format, Output::Raw)
                .unwrap();
            let bytes = target.read(&gpu).unwrap();
            let expected: &[f32] = bytemuck::cast_slice(&bytes);
            let mut worst = (0.0f32, 0usize);
            for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                if (a - b).abs() > worst.0 {
                    worst = ((a - b).abs(), i / 4);
                }
            }
            let k = worst.1 * 4;
            assert!(
                worst.0 < 0.02,
                "shared={shared_top} zoom={zoom} level={level} {what}: error {} at ({}, {}): {:?} vs {:?}",
                worst.0,
                worst.1 as u32 % screen,
                worst.1 as u32 / screen,
                &actual[k..k + 4],
                &expected[k..k + 4],
            );
        };
        check(&mut engine, &doc, "initial");
        for pass in 0..2 {
            let source = engine
                .canvas
                .sources
                .iter()
                .position(|s| s.node == Some(top))
                .unwrap();
            let brush = test_brush(40.0);
            let base = match &doc.node(top).unwrap().kind {
                NodeKind::Raster { raster, .. } => raster.clone(),
                _ => unreachable!(),
            };
            let mut cpu = emulsion_raster::paint::Stroke::new(
                base,
                brush,
                emulsion_raster::paint::Ink::Color(crate::brush::INK),
                None,
            );
            let mut stroke = GpuStroke::begin(source, brush);
            let mut shown = doc.clone();
            for i in 0..12 {
                let (x, y) = (
                    30.0 + i as f32 * 38.0,
                    200.0 + pass as f32 * 150.0 + (i as f32 * 0.3).sin() * 80.0,
                );
                stroke.point(x, y);
                cpu.point_at(x, y, None, Some(i as f64));
                let (b, c, a, e) = engine.brush_parts();
                stroke.render(b, c, a, e).unwrap();
                engine.flush();
                if let NodeKind::Raster { raster, .. } = &mut shown.node_mut(top).unwrap().kind {
                    *raster = Arc::new(cpu.render(raster).0);
                }
                // Each check builds a reference engine; sample the stroke.
                if i % 4 == 0 {
                    check(&mut engine, &shown, &format!("pass {pass} frame {i}"));
                }
            }
            let (_, _, atlas, encoder) = engine.brush_parts();
            let readback = stroke.finish(atlas, encoder);
            engine.flush();
            readback.map();
            gpu.wait();
            let raster = readback
                .complete(&gpu, &mut engine.canvas, &mut engine.atlas, source)
                .unwrap();
            if let NodeKind::Raster { raster: r, .. } = &mut doc.node_mut(top).unwrap().kind {
                *r = Arc::new((*raster).clone());
            }
            engine.reload(&doc, None, true).unwrap();
            check(&mut engine, &doc, &format!("pass {pass} after commit"));
        }
    }
}

#[test]
fn vector_mask_changes_invalidate_bakes_and_exclude_unmasked_vector_fast_path() {
    use emulsion_core::{Command, Node, VectorMask};
    use emulsion_raster::vector::{Path, PathStyle};
    let mut doc = Document::new(40, 32);
    let path = Arc::new(Path::from_svg("M 2 2 L 30 2 L 30 26 L 2 26 Z").unwrap());
    let mut node = Node::path(1, "Masked vector", path, PathStyle::default(), 40, 32);
    node.vector_mask = Some(VectorMask {
        path: Arc::new(Path::from_svg("M 4 4 L 18 4 L 18 20 L 4 20 Z").unwrap()),
        ..Default::default()
    });
    doc.nodes.push(node);
    doc.next_id = 2;
    assert!(!vector_nodes(&doc).contains_key(&1));
    let tree = doc.composite_tree();
    let previous = cached(&tree.nodes[0], 40, 32);
    for command in [
        Command::SetVectorMaskInverted {
            id: 1,
            inverted: true,
        },
        Command::SetVectorMaskProperties {
            id: 1,
            properties: emulsion_core::MaskProperties {
                density: 0.5,
                feather: 2.,
            },
        },
        Command::SetVectorMaskTransform {
            id: 1,
            transform: [1., 0., 0., 1., 3., 2.],
        },
        Command::SetVectorMaskEnabled {
            id: 1,
            enabled: false,
        },
    ] {
        command.apply(&mut doc).unwrap();
        let tree = doc.composite_tree();
        assert_pixels_equal(
            &bake(&tree.nodes[0], 40, 32, BlendSpace::Linear, Some(&previous)),
            &bake(&tree.nodes[0], 40, 32, BlendSpace::Linear, None),
        );
    }
}
