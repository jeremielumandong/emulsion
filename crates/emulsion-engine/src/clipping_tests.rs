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
    let slots: Vec<_> = compiler
        .ops
        .iter()
        .map(|op| match op {
            Op::Fill { clip, alpha, .. } => (*clip, *alpha),
            _ => panic!("unexpected op {op:?}"),
        })
        .collect();
    assert_eq!(slots, [(NONE, 0), (0, NONE), (0, NONE)]);
    assert_eq!(compiler.alpha_slots, 1);
}

#[test]
fn clipping_chain_visibility_depends_on_root_not_middle() {
    let doc = Document::new(32, 16);
    let mut nodes = [fill(1, None), fill(2, Some(0)), fill(3, Some(1))];
    nodes[1].visible = false;
    let compiler = compile(&doc, &nodes);
    assert_eq!(compiler.ops.len(), 2, "hidden middle must not hide top");
    assert!(matches!(compiler.ops[1], Op::Fill { clip: 0, .. }));
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
        compiler
            .unsupported
            .iter()
            .all(|reason| reason.starts_with("grouped clipping ")),
        "{:?}",
        compiler.unsupported
    );
    assert_eq!(compiler.unsupported.len(), 2);
    assert_eq!(compiler.alpha_slots, 2, "long chains need one slot each");
    let clips: Vec<_> = compiler
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Fill { clip, .. } if *clip != NONE => Some(*clip),
            _ => None,
        })
        .collect();
    assert_eq!(clips, [vec![0; 19], vec![1; 19]].concat());

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
fn clipping_chain_cpu_fallback_through_visibility_release_undo_and_pixel_edits() {
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
