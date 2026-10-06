//! Clipping-chain parity between the native GPU canvas and CPU export.
use super::*;
use crate::{Camera, Engine, Gpu, Offscreen, Output, vector::VectorSpace};
use emulsion_core::{Command, Node, command::Slot as NodeSlot};
use emulsion_raster::{Placement, color::px_to_f};

fn fill(id: u64, clip_to: Option<usize>) -> CompositeNode {
    CompositeNode {
        id,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        blending: Default::default(),
        mask: None,
        clip_to,
        clip_rect: None,
        content: NodeContent::Fill([0.2, 0.1, 0.3, 0.5]),
    }
}

fn compile<'a>(doc: &'a Document, nodes: &[CompositeNode]) -> Compiler<'a> {
    let mut compiler = Compiler {
        vectors: HashMap::new(),
        names: HashMap::new(),
        width: doc.width,
        height: doc.height,
        space: doc.blend_space,
        sources: Vec::new(),
        ops: Vec::new(),
        runs: Vec::new(),
        open_run: None,
        in_clip_envelope: false,
        unsupported: Vec::new(),
        rasterized: Vec::new(),
        alpha_slots: 0,
        paint: None,
        paint_node: None,
        baked_prev: Vec::new(),
        baked_new: Vec::new(),
        _doc: doc,
    };
    compiler.list(nodes, 0);
    compiler
}

#[test]
fn clipping_chain_compiles_every_member_against_the_root() {
    let doc = Document::new(32, 16);
    // Three native Photo layers, each clipped to the preceding sibling.
    let nodes = [fill(1, None), fill(2, Some(0)), fill(3, Some(1))];
    let compiler = compile(&doc, &nodes);
    assert!(
        compiler.unsupported.is_empty(),
        "{:?}",
        compiler.unsupported
    );
    assert!(matches!(compiler.ops[0], Op::Push { isolated: true }));
    assert!(matches!(compiler.ops[2], Op::NormalizeClip { alpha: 0 }));
    assert!(matches!(
        compiler.ops[5],
        Op::Pop {
            clip: 0,
            alpha: NONE,
            ..
        }
    ));
    let slots: Vec<_> = compiler
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Fill { clip, alpha, .. } => Some((*clip, *alpha)),
            _ => None,
        })
        .collect();
    assert_eq!(
        slots,
        [(NONE, NONE); 3],
        "members do not reapply the root shape"
    );
    assert_eq!(compiler.alpha_slots, 1);
}

#[test]
fn clipping_chain_visibility_depends_on_root_not_middle() {
    let doc = Document::new(32, 16);
    let mut nodes = [fill(1, None), fill(2, Some(0)), fill(3, Some(1))];
    nodes[1].visible = false;
    let compiler = compile(&doc, &nodes);
    assert_eq!(compiler.ops.len(), 5, "hidden middle must not hide top");
    assert!(matches!(compiler.ops[3], Op::Fill { clip: NONE, .. }));
    nodes[0].visible = false;
    nodes[1].visible = true;
    assert!(
        compile(&doc, &nodes).ops.is_empty(),
        "hidden root hides chain"
    );
}

#[test]
fn clipping_chain_slots_are_scoped_to_sibling_roots() {
    let doc = Document::new(32, 16);
    let chain = || {
        (0_usize..20)
            .map(|i| fill(i as u64 + 1, i.checked_sub(1)))
            .collect::<Vec<_>>()
    };
    let mut group = fill(50, None);
    group.content = NodeContent::Group(chain());
    group.opacity = 0.7; // Keep the isolation boundary in the op program.
    let mut nodes = chain();
    nodes.push(group);
    let compiler = compile(&doc, &nodes);
    assert!(
        compiler.unsupported.is_empty(),
        "{:?}",
        compiler.unsupported
    );
    assert_eq!(compiler.alpha_slots, 2, "long chains need one slot each");
    let shapes: Vec<_> = compiler
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::NormalizeClip { alpha } => Some(*alpha),
            _ => None,
        })
        .collect();
    assert_eq!(shapes, [0, 1]);
    assert!(
        compiler
            .ops
            .iter()
            .all(|op| !matches!(op, Op::Fill { clip, .. } if *clip != NONE))
    );

    // Invalid/self/forward links are ignored just as in the CPU compositor.
    let nodes = [
        fill(1, Some(usize::MAX)),
        fill(2, Some(1)),
        fill(3, Some(0)),
    ];
    let compiler = compile(&doc, &nodes);
    assert_eq!(compiler.alpha_slots, 1);
    assert!(matches!(compiler.ops[1], Op::Fill { clip: NONE, .. }));
    assert!(matches!(compiler.ops[2], Op::Fill { clip: 0, .. }));
}

fn add(doc: &mut Document, raster: Raster) -> NodeId {
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Clip member",
            Arc::new(raster),
            Placement::default(),
        )),
        slot: NodeSlot::TOP,
    }
    .apply(doc)
    .unwrap()
    .unwrap()
}

fn fixture(space: BlendSpace, group_blend: Option<BlendMode>) -> (Document, [NodeId; 3]) {
    let mut doc = Document::new(32, 16);
    doc.blend_space = space;
    let base = add(
        &mut doc,
        Raster::from_srgba8(
            32,
            16,
            &(0..512)
                .flat_map(|i| [40, 100, 180, [255, 128, 0, 0][i % 32 / 8]])
                .collect::<Vec<_>>(),
        ),
    );
    let middle = add(&mut doc, Raster::solid(32, 16, [0.1, 0.7, 0.2, 1.0]));
    let top = add(&mut doc, Raster::solid(32, 16, [0.8, 0.1, 0.6, 1.0]));
    for (id, clip_to) in [(middle, base), (top, middle)] {
        Command::SetClip {
            id,
            clip_to: Some(clip_to),
        }
        .apply(&mut doc)
        .unwrap();
    }
    if let Some(blend) = group_blend {
        doc.node_mut(middle).unwrap().opacity = 0.42;
        doc.node_mut(middle).unwrap().blend = BlendMode::Multiply;
        doc.node_mut(top).unwrap().opacity = 0.73;
        doc.node_mut(top).unwrap().blend = BlendMode::Screen;
        let group = Command::Group {
            ids: vec![base, middle, top],
            name: "Clipping group".into(),
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        doc.node_mut(group).unwrap().blend = blend;
        doc.node_mut(group).unwrap().opacity = 0.7;
    }
    (doc, [base, middle, top])
}

fn render_and_check(engine: &mut Engine, doc: &Document) -> Vec<[f32; 4]> {
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
        // Exercise the CPU-composited image through real software-GPU texture
        // presentation. Never render the ineligible per-member op program.
        let mut flat = Document::new(doc.width, doc.height);
        flat.nodes.push(Node::raster(
            1,
            "CPU grouped-clipping fallback",
            Arc::new(flatten(&doc.composite_tree(), 0)),
            Placement::default(),
        ));
        fallback = Engine::new(
            engine.gpu.clone(),
            &flat,
            None,
            VectorSpace::Srgb,
            false,
            false,
            (32, 16),
        )
        .unwrap();
        &mut fallback
    };
    engine.camera = Camera {
        center: [16.0, 8.0],
        zoom: 1.0,
    };
    let target = Offscreen::new(&engine.gpu, (32, 16), wgpu::TextureFormat::Rgba32Float);
    engine
        .render(&target.view, target.format, Output::Raw)
        .unwrap();
    let pixels: Vec<[f32; 4]> = target
        .read(&engine.gpu)
        .unwrap()
        .as_chunks::<16>()
        .0
        .iter()
        .map(|bytes| {
            std::array::from_fn(|c| f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap()))
        })
        .collect();
    let expected = flatten(&doc.composite_tree(), 0);
    for (i, pixel) in pixels.iter().enumerate() {
        let reference = px_to_f(expected.get(i as u32 % 32, i as u32 / 32));
        for c in 0..4 {
            assert!(
                (pixel[c] - reference[c]).abs() < 0.002,
                "pixel {i} channel {c}: GPU {pixel:?}, CPU {reference:?}"
            );
        }
    }
    pixels
}

#[test]
fn clipping_chain_gpu_through_visibility_release_undo_and_pixel_edits() {
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
    for space in [BlendSpace::Linear, BlendSpace::Srgb] {
        for group in [None, Some(BlendMode::Normal), Some(BlendMode::PassThrough)] {
            let (original, [base, middle, top]) = fixture(space, group);
            for cache in [false, true] {
                let mut doc = original.clone();
                let mut engine = Engine::new(
                    gpu.clone(),
                    &doc,
                    None,
                    VectorSpace::Srgb,
                    false,
                    cache,
                    (32, 16),
                )
                .unwrap();
                assert_eq!(engine.cache.is_some(), cache);
                assert!(
                    engine.canvas.unsupported.is_empty(),
                    "{:?}",
                    engine.canvas.unsupported
                );
                assert_eq!(engine.canvas.cacheable_prefix(), engine.canvas.ops.len());
                for _ in 0..2 {
                    let pixels = render_and_check(&mut engine, &doc);
                    assert!(
                        pixels[24][3] < 0.0001,
                        "opaque middle/top must not expand root shape"
                    );
                }
                // Release, then restore the authored chain as undo/reopen does.
                for id in [middle, top] {
                    Command::SetClip { id, clip_to: None }
                        .apply(&mut doc)
                        .unwrap();
                }
                engine.reload(&doc, None, false).unwrap();
                assert!(render_and_check(&mut engine, &doc)[24][3] > 0.5);
                doc = original.clone();
                engine.reload(&doc, None, false).unwrap();
                assert!(render_and_check(&mut engine, &doc)[24][3] < 0.0001);
                // A hidden intermediate does not change the root of later members.
                doc.node_mut(middle).unwrap().visible = false;
                engine.reload(&doc, None, false).unwrap();
                assert!(render_and_check(&mut engine, &doc)[0][3] > 0.69);
                doc.node_mut(base).unwrap().visible = false;
                engine.reload(&doc, None, false).unwrap();
                assert!(
                    render_and_check(&mut engine, &doc)
                        .iter()
                        .all(|p| p[3] < 0.0001)
                );
                // Pixel-only root edits must invalidate the populated composite cache.
                doc = original.clone();
                engine.reload(&doc, None, false).unwrap();
                render_and_check(&mut engine, &doc);
                let tables = engine.canvas.tables.clone();
                let NodeKind::Raster { raster, .. } = &mut doc.node_mut(base).unwrap().kind else {
                    unreachable!()
                };
                *raster = Arc::new(Raster::solid(32, 16, [0.2, 0.1, 0.3, 1.0]));
                engine.reload(&doc, None, false).unwrap();
                assert_eq!(
                    engine.canvas.tables, tables,
                    "pixel edit should keep atlas tables"
                );
                assert!(render_and_check(&mut engine, &doc)[24][3] > 0.69);
                engine.reload(&original, None, false).unwrap();
                assert!(render_and_check(&mut engine, &original)[24][3] < 0.0001);
            }
        }
    }
}

#[test]
fn pass_through_clipping_base_compiles_an_independent_shape() {
    let doc = Document::new(32, 16);
    let mut group = fill(1, None);
    group.blend = BlendMode::PassThrough;
    group.content = NodeContent::Group(vec![fill(2, None)]);
    let compiler = compile(&doc, &[group, fill(3, Some(0))]);
    assert!(
        compiler.ops.iter().any(|op| matches!(
            op,
            Op::Pop {
                isolated: true,
                opacity: 0.0,
                alpha: 0,
                ..
            }
        )),
        "pass-through base must save its shape without painting it: {:?}",
        compiler.ops
    );
    assert_eq!(
        compiler
            .ops
            .iter()
            .filter(|op| matches!(op, Op::Fill { .. }))
            .count(),
        3,
        "children must be evaluated against transparency and the real backdrop"
    );
}

#[test]
fn pass_through_clipping_base_cpu_fallback_with_masks_vectors_and_reload() {
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
    for space in [BlendSpace::Linear, BlendSpace::Srgb] {
        for vector in [false, true] {
            let (mut doc, [base, middle, top]) = fixture(space, None);
            for id in [middle, top] {
                doc.node_mut(id).unwrap().clip_to = None;
            }
            let group = Command::Group {
                ids: vec![base],
                name: "Pass-through clip base".into(),
            }
            .apply(&mut doc)
            .unwrap()
            .unwrap();
            let node = doc.node_mut(group).unwrap();
            node.blend = BlendMode::PassThrough;
            node.opacity = 0.37;
            node.mask = Some(Arc::new(emulsion_raster::Mask::empty(32, 16, 127)));
            for id in [middle, top] {
                doc.node_mut(id).unwrap().clip_to = Some(group);
            }
            doc.node_mut(middle).unwrap().blend = BlendMode::Multiply;
            doc.node_mut(top).unwrap().opacity = 0.71;
            if vector {
                doc.node_mut(base).unwrap().kind = Node::path(
                    base,
                    "Native clip shape",
                    Arc::new(emulsion_raster::vector_geometry::rectangle(
                        0., 0., 16., 16.,
                    )),
                    PathStyle {
                        fill: Some([120, 50, 200, 255]),
                        stroke: None,
                        ..Default::default()
                    },
                    32,
                    16,
                )
                .kind;
            }
            Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Translucent backdrop",
                    Arc::new(Raster::solid(32, 16, [0.05, 0.15, 0.1, 0.25])),
                    Placement::default(),
                )),
                slot: NodeSlot {
                    parent: None,
                    index: 0,
                },
            }
            .apply(&mut doc)
            .unwrap();
            let original = doc.clone();
            for cache in [false, true] {
                let mut doc = original.clone();
                let mut engine = Engine::new(
                    gpu.clone(),
                    &doc,
                    None,
                    VectorSpace::Srgb,
                    vector,
                    cache,
                    (32, 16),
                )
                .unwrap();
                if vector {
                    assert!(engine.canvas.rasterized.is_empty());
                    assert_eq!(
                        engine.canvas.runs.len(),
                        1,
                        "shape and appearance must reuse the native vector run"
                    );
                }
                for _ in 0..2 {
                    assert!(
                        (render_and_check(&mut engine, &doc)[24][3] - 0.25).abs() < 0.002,
                        "clipped overlays cannot use backdrop alpha as the group's shape"
                    );
                }
                if vector {
                    Command::TranslateNode {
                        id: base,
                        dx: 4.0,
                        dy: 0.0,
                    }
                    .apply(&mut doc)
                    .unwrap();
                } else {
                    let NodeKind::Raster { raster, .. } = &mut doc.node_mut(base).unwrap().kind
                    else {
                        unreachable!()
                    };
                    *raster = Arc::new(Raster::solid(32, 16, [0.4, 0.2, 0.1, 0.6]));
                }
                engine.reload(&doc, None, vector).unwrap();
                render_and_check(&mut engine, &doc);
                doc = original.clone();
                engine.reload(&doc, None, vector).unwrap();
                render_and_check(&mut engine, &doc);
                doc.node_mut(group).unwrap().mask =
                    Some(Arc::new(emulsion_raster::Mask::empty(32, 16, 200)));
                engine.reload(&doc, None, vector).unwrap();
                render_and_check(&mut engine, &doc);
                doc.node_mut(group).unwrap().visible = false;
                engine.reload(&doc, None, vector).unwrap();
                render_and_check(&mut engine, &doc);
                engine.reload(&original, None, vector).unwrap();
                render_and_check(&mut engine, &original);
            }
        }
    }
}

#[test]
fn grouped_clip_limits_and_advanced_features_stay_on_cpu() {
    let doc = Document::new(32, 16);
    let mut hidden_gap = fill(2, None);
    hidden_gap.visible = false;
    let nodes = [fill(1, None), hidden_gap, fill(3, Some(0))];
    assert!(!compile(&doc, &nodes).unsupported.is_empty());
    for blend in [BlendMode::PassThrough, BlendMode::Dissolve] {
        let mut root = fill(1, None);
        root.blend = blend;
        if blend == BlendMode::PassThrough {
            root.content = NodeContent::Group(vec![fill(2, None)]);
        }
        assert!(
            !compile(&doc, &[root, fill(3, Some(0))])
                .unsupported
                .is_empty()
        );
    }
    for fill_opacity in [0.0, 0.5] {
        let mut root = fill(1, None);
        root.blending.fill_opacity = fill_opacity;
        assert!(
            !compile(&doc, &[root, fill(2, Some(0))])
                .unsupported
                .is_empty()
        );
    }
    let mut nodes = vec![fill(1, None)];
    for _ in 0..8 {
        let mut root = fill(1, None);
        root.content = NodeContent::Group(nodes);
        nodes = vec![root, fill(2, Some(0))];
    }
    assert!(
        !compile(&doc, &nodes).unsupported.is_empty(),
        "envelopes consume shader stack depth"
    );
    let mut nodes = Vec::new();
    for i in 0..17 {
        nodes.push(fill(2 * i + 1, None));
        nodes.push(fill(2 * i + 2, Some(2 * i as usize)));
    }
    assert!(
        !compile(&doc, &nodes).unsupported.is_empty(),
        "alpha-slot limit must fall back"
    );
}

#[test]
fn masked_group_clip_envelopes_nest_and_keep_vector_cache_boundaries() {
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
    for space in [BlendSpace::Linear, BlendSpace::Srgb] {
        for vector in [false, true] {
            let mut doc = Document::new(32, 16);
            doc.blend_space = space;
            let pixel = |id, color| {
                Node::raster(
                    id,
                    "Clip fixture",
                    Arc::new(Raster::solid(32, 16, color)),
                    Placement::default(),
                )
            };
            let backdrop = pixel(1, [0.05, 0.15, 0.1, 0.25]);
            let mut child = pixel(11, [0.12, 0.06, 0.24, 0.4]);
            child.parent = Some(10);
            child.mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
                32,
                16,
                0,
                |x, _| [255, 128, 0, 0][x as usize / 8],
            )));
            let mut inner = pixel(12, [0.1, 0.3, 0.05, 0.5]);
            inner.parent = Some(10);
            inner.clip_to = Some(11);
            inner.blend = BlendMode::Screen;
            inner.opacity = 0.61;
            let mut root = Node::group(10, "Masked clipping root");
            root.blend = BlendMode::Multiply;
            root.opacity = 0.47;
            root.mask = Some(Arc::new(emulsion_raster::Mask::empty(32, 16, 173)));
            let mut child_member = if vector {
                Node::path(
                    21,
                    "Native vector member",
                    Arc::new(emulsion_raster::vector_geometry::rectangle(
                        0., 0., 32., 16.,
                    )),
                    PathStyle {
                        fill: Some([180, 50, 120, 255]),
                        stroke: None,
                        ..Default::default()
                    },
                    32,
                    16,
                )
            } else {
                pixel(21, [0.25, 0.1, 0.15, 0.5])
            };
            child_member.parent = Some(20);
            let mut member = Node::group(20, "Masked clipping member");
            member.blend = BlendMode::SoftLight;
            member.opacity = 0.63;
            member.mask = Some(Arc::new(emulsion_raster::Mask::empty(32, 16, 139)));
            member.clip_to = Some(10);
            let mut top = pixel(30, [0.15, 0.08, 0.2, 0.3]);
            top.blend = BlendMode::Color;
            top.opacity = 0.31;
            top.clip_to = Some(20);
            doc.nodes = vec![backdrop, child, inner, root, child_member, member, top];
            for cache in [false, true] {
                let mut engine = Engine::new(
                    gpu.clone(),
                    &doc,
                    None,
                    VectorSpace::Srgb,
                    vector,
                    cache,
                    (32, 16),
                )
                .unwrap();
                assert!(
                    engine.canvas.unsupported.is_empty(),
                    "{:?}",
                    engine.canvas.unsupported
                );
                let normalization: Vec<_> = engine
                    .canvas
                    .ops
                    .iter()
                    .enumerate()
                    .filter_map(|(i, op)| match op {
                        Op::NormalizeClip { alpha } => Some((i, *alpha)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    normalization.len(),
                    2,
                    "root and inner stack must save independent shapes"
                );
                let words = engine.canvas.program();
                for (i, alpha) in normalization {
                    assert_eq!(words[HEADER_WORDS + i * OP_WORDS], 10);
                    assert_eq!(words[HEADER_WORDS + i * OP_WORDS + 2], alpha);
                }
                assert_eq!(
                    engine.canvas.cacheable_prefix(),
                    if vector { 1 } else { engine.canvas.ops.len() },
                    "cache cannot split an envelope before a vector member"
                );
                for _ in 0..2 {
                    let pixels = render_and_check(&mut engine, &doc);
                    assert!(
                        (pixels[24][3] - 0.25).abs() < 0.002,
                        "members cannot grow root shape"
                    );
                }
                let mut changed = doc.clone();
                changed.node_mut(10).unwrap().opacity = 0.0;
                engine.reload(&changed, None, vector).unwrap();
                let pixels = render_and_check(&mut engine, &changed);
                assert!(
                    (pixels[0][3] - 0.25).abs() < 0.002,
                    "root opacity envelopes every member"
                );
                changed.node_mut(10).unwrap().opacity = 1.0;
                changed.node_mut(20).unwrap().mask =
                    Some(Arc::new(emulsion_raster::Mask::empty(32, 16, 211)));
                engine.reload(&changed, None, vector).unwrap();
                assert!(engine.canvas.unsupported.is_empty());
                render_and_check(&mut engine, &changed);
            }
        }
    }
}

fn fractional_vector_stack(nested: bool) -> Document {
    let mut doc = Document::new(32, 16);
    let root = Node::raster(
        1,
        "Half-alpha clipping root",
        Arc::new(Raster::solid(32, 16, [0.2, 0.1, 0.3, 0.5])),
        Placement::default(),
    );
    let path = |id, x, color| {
        Node::path(
            id,
            "Fractional vector member",
            Arc::new(emulsion_raster::vector_geometry::rectangle(
                x,
                0.,
                32. - x,
                16.,
            )),
            PathStyle {
                fill: Some(color),
                stroke: None,
                ..Default::default()
            },
            32,
            16,
        )
    };
    let mut white = path(3, 0., [255; 4]);
    let mut black = path(4, 0.5, [0, 0, 0, 255]);
    if nested {
        let mut group = Node::group(2, "Isolated vector member");
        group.blend = BlendMode::Normal;
        group.clip_to = Some(1);
        white.parent = Some(2);
        black.parent = Some(2);
        doc.nodes = vec![root, white, black, group];
    } else {
        white.clip_to = Some(1);
        black.clip_to = Some(3);
        doc.nodes = vec![root, white, black];
    }
    doc
}

#[test]
fn clipping_envelopes_keep_vector_layers_separate_and_restore_merge_scope() {
    for nested in [false, true] {
        let mut doc = fractional_vector_stack(nested);
        for id in [5, 6] {
            doc.nodes.push(Node::path(
                id,
                "Unclipped vectors may still merge",
                Arc::new(emulsion_raster::vector_geometry::rectangle(
                    16., 0., 16., 16.,
                )),
                PathStyle {
                    fill: Some([255; 4]),
                    stroke: None,
                    ..Default::default()
                },
                32,
                16,
            ));
        }
        let mut compiler = compile(&doc, &[]);
        compiler.vectors = vector_nodes(&doc);
        compiler.list(&doc.composite_tree().nodes, 0);
        assert!(
            compiler.unsupported.is_empty(),
            "{:?}",
            compiler.unsupported
        );
        assert_eq!(
            compiler.runs.iter().map(Vec::len).collect::<Vec<_>>(),
            [1, 1, 2],
            "only layers outside the envelope may share a Vello run"
        );
        assert!(
            !compiler.in_clip_envelope,
            "compilation must restore its enclosing scope"
        );
    }
}

#[test]
fn fractional_vector_members_match_linear_cpu_interpolation_on_gpu() {
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
    for nested in [false, true] {
        for space in [BlendSpace::Linear, BlendSpace::Srgb] {
            let mut doc = fractional_vector_stack(nested);
            doc.blend_space = space;
            for cache in [false, true] {
                let mut engine = Engine::new(
                    gpu.clone(),
                    &doc,
                    None,
                    VectorSpace::Srgb,
                    true,
                    cache,
                    (32, 16),
                )
                .unwrap();
                assert!(
                    engine.canvas.unsupported.is_empty(),
                    "{:?}",
                    engine.canvas.unsupported
                );
                assert_eq!(
                    engine.canvas.runs.iter().map(Vec::len).collect::<Vec<_>>(),
                    [1, 1]
                );
                assert_eq!(
                    engine.canvas.cacheable_prefix(),
                    0,
                    "vector members prevent caching a partial envelope"
                );
                for _ in 0..2 {
                    let pixels = render_and_check(&mut engine, &doc);
                    assert!(
                        (pixels[0][0] - 0.25).abs() < 0.002,
                        "half-covered black over white must interpolate in linear space: {:?}",
                        pixels[0]
                    );
                    assert!(
                        (pixels[0][3] - 0.5).abs() < 0.002,
                        "members retain the root alpha"
                    );
                }
            }
        }
    }
}
