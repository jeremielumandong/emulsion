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
