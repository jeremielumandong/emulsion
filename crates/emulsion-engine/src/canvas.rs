//! Compile an Emulsion document into what the GPU compositor runs: tiled
//! raster sources in the atlas, a per-pixel op program (the same op model as
//! `emulsion-gpu`'s tile compositor), and runs of Vello-drawn vector nodes.
//!
//! Anything the spike does not implement is listed in `unsupported` and
//! skipped, so fidelity numbers are never silently flattered.

use crate::atlas::{Atlas, Slot};
use emulsion_core::{Document, NodeId, NodeKind};
use emulsion_raster::blend::{BlendMode, BlendSpace};
use emulsion_raster::composite::{CompositeNode, CompositeTree, NodeContent, flatten};
use emulsion_raster::vector::{Path, PathPaint, PathStyle, StrokeAlignment};
use emulsion_raster::{IRect, Raster, TILE, TileCoord};
use std::collections::HashMap;
use std::sync::Arc;

pub const NONE: u32 = u32::MAX;
pub const OP_WORDS: usize = 16;
pub const HEADER_WORDS: usize = 8;
/// Clip-base alpha slots per pixel in the shader.
pub const MAX_ALPHA_SLOTS: u32 = 16;
/// Group nesting per pixel in the shader.
pub const MAX_DEPTH: usize = 8;

/// The composite shader's blend-mode numbering (from `emulsion-gpu`).
pub fn mode(mode: BlendMode) -> u32 {
    use BlendMode::*;
    match mode {
        Normal | PassThrough | Dissolve => 0,
        Darken => 1,
        Multiply => 2,
        ColorBurn => 3,
        LinearBurn => 4,
        Lighten => 5,
        Screen => 6,
        ColorDodge => 7,
        LinearDodge => 8,
        Overlay => 9,
        SoftLight => 10,
        HardLight => 11,
        VividLight => 12,
        LinearLight => 13,
        PinLight => 14,
        HardMix => 15,
        Difference => 16,
        Exclusion => 17,
        Subtract => 18,
        Divide => 19,
        DarkerColor => 20,
        LighterColor => 21,
        Hue => 22,
        Saturation => 23,
        Color => 24,
        Luminosity => 25,
    }
}

/// What a baked raster was flattened from.
///
/// Baking runs `flatten` over a whole document-sized layer, which for a 4K
/// document is tens of milliseconds. A reload re-walks the tree, so without
/// this every masked or placed layer is re-baked on every edit. Identical
/// inputs give identical pixels, so the previous `Arc` is reused -- which also
/// keeps its atlas slots, and therefore keeps the composite cache valid.
#[derive(Clone, PartialEq)]
pub(crate) struct BakeKey {
    node: u64,
    /// Distinguishes a group's mask shape from the group's own content.
    mask_shape: bool,
    /// `Arc` address of the source raster, or 0.
    raster: usize,
    /// `Arc` address of the mask, or 0.
    mask: usize,
    placement: Option<emulsion_raster::composite::Placement>,
    fill: Option<[u32; 4]>,
}

/// A document-aligned raster whose tiles live in the atlas.
pub struct Source {
    pub name: String,
    /// The document node whose raster this is, when the node compiles to a
    /// direct source. `None` for baked sources, whose pixels are a function of
    /// a mask or placement rather than the raster alone, and so cannot be
    /// swapped by [`Canvas::replace_raster`].
    pub node: Option<NodeId>,
    pub raster: Arc<Raster>,
    pub tiles_x: u32,
    pub tiles_y: u32,
    pub table_offset: u32,
    pub slots: Vec<Option<Slot>>,
}

impl Source {
    pub fn index(&self, c: TileCoord) -> Option<usize> {
        (c.x >= 0 && c.y >= 0 && (c.x as u32) < self.tiles_x && (c.y as u32) < self.tiles_y)
            .then(|| c.y as usize * self.tiles_x as usize + c.x as usize)
    }
}

#[derive(Clone, Debug)]
pub enum VectorKind {
    Path {
        path: Arc<Path>,
        style: PathStyle,
    },
    Text {
        spec: Arc<emulsion_core::text::TextSpec>,
    },
}

#[derive(Clone, Debug)]
pub struct VectorItem {
    pub node: NodeId,
    pub kind: VectorKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Source {
        mode: u32,
        source: usize,
        opacity: f32,
        clip: u32,
        alpha: u32,
    },
    Fill {
        mode: u32,
        color: [f32; 4],
        opacity: f32,
        clip: u32,
        alpha: u32,
    },
    Vector {
        mode: u32,
        run: usize,
        opacity: f32,
        clip: u32,
        alpha: u32,
    },
    Push {
        isolated: bool,
    },
    Pop {
        isolated: bool,
        mode: u32,
        opacity: f32,
        clip: u32,
        alpha: u32,
        mask: Option<usize>,
    },
}

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub space: BlendSpace,
    pub sources: Vec<Source>,
    pub ops: Vec<Op>,
    /// Runs of consecutive vector nodes, each drawn by Vello into one target.
    pub runs: Vec<Vec<VectorItem>>,
    pub unsupported: Vec<String>,
    /// Source that brush strokes paint into.
    pub paint: Option<usize>,
    table: Vec<u32>,
    pub tables: wgpu::Buffer,
    /// Document rectangles whose pixels changed since the composite cache
    /// last looked.
    pub dirty: Vec<IRect>,
    /// Baked rasters and the inputs they came from, so a reload can reuse
    /// those whose inputs did not change.
    pub(crate) baked: Vec<(BakeKey, Arc<Raster>)>,
}

/// Which document nodes Vello can draw, and how.
fn vector_nodes(doc: &Document) -> HashMap<NodeId, VectorKind> {
    doc.nodes
        .iter()
        .filter(|n| n.mask.is_none() && n.blending == Default::default())
        .filter_map(|n| match &n.kind {
            NodeKind::Path { path, style, .. } if path_supported(style) => Some((
                n.id,
                VectorKind::Path {
                    path: path.clone(),
                    style: *style,
                },
            )),
            NodeKind::Text { spec, .. } if text_supported(spec) => {
                Some((n.id, VectorKind::Text { spec: spec.clone() }))
            }
            _ => None,
        })
        .collect()
}

pub fn path_supported(style: &PathStyle) -> bool {
    matches!(style.fill_paint, PathPaint::Solid)
        && matches!(style.stroke_paint, PathPaint::Solid)
        && style.alignment == StrokeAlignment::Center
}

pub fn text_supported(spec: &emulsion_core::text::TextSpec) -> bool {
    spec.warp.is_identity()
        && spec.text_path.is_none()
        && !spec.vertical
        && spec.runs.is_empty()
        && spec.height.is_none()
        && spec.anti_alias == emulsion_core::text::AntiAliasMode::Smooth
}

struct Compiler<'a> {
    vectors: HashMap<NodeId, VectorKind>,
    names: HashMap<NodeId, String>,
    width: u32,
    height: u32,
    space: BlendSpace,
    sources: Vec<(String, Arc<Raster>, Option<NodeId>)>,
    ops: Vec<Op>,
    runs: Vec<Vec<VectorItem>>,
    /// Index into `ops` of the open run's op, if the last op is a mergeable run.
    open_run: Option<usize>,
    unsupported: Vec<String>,
    alpha_slots: u32,
    paint: Option<(NodeId, usize)>,
    paint_node: Option<NodeId>,
    /// Bakes from the previous compile, reused when their inputs match.
    baked_prev: Vec<(BakeKey, Arc<Raster>)>,
    /// Bakes this compile produced, kept for the next one.
    baked_new: Vec<(BakeKey, Arc<Raster>)>,
    _doc: &'a Document,
}

impl Compiler<'_> {
    fn note(&mut self, what: String) {
        tracing::warn!("unsupported: {what}");
        self.unsupported.push(what);
    }

    fn name(&self, id: NodeId) -> String {
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("#{id}"))
    }

    fn source(&mut self, name: String, raster: Arc<Raster>) -> usize {
        self.sources.push((name, raster, None));
        self.sources.len() - 1
    }

    /// A source that is exactly one node's raster, so a later edit can swap
    /// its pixels instead of recompiling.
    fn direct_source(&mut self, name: String, raster: Arc<Raster>, node: NodeId) -> usize {
        self.sources.push((name, raster, Some(node)));
        self.sources.len() - 1
    }

    /// Render one node's content alone (placement, mask) to a doc-aligned raster.
    /// What this bake depends on: identical inputs give identical pixels.
    fn bake_key(node: &CompositeNode, mask_shape: bool) -> BakeKey {
        let (raster, placement, fill) = match &node.content {
            NodeContent::Pixels { raster, placement } => (raster.id(), Some(*placement), None),
            NodeContent::Fill(c) => (0, None, Some(c.map(f32::to_bits))),
            _ => (0, None, None),
        };
        BakeKey {
            node: node.id,
            mask_shape,
            raster,
            mask: node
                .mask
                .as_ref()
                .map_or(0, |m| Arc::as_ptr(m) as *const u8 as usize),
            placement,
            fill,
        }
    }

    fn bake_cached(&mut self, node: &CompositeNode, mask_shape: bool) -> Arc<Raster> {
        let key = Self::bake_key(node, mask_shape);
        let hit = self
            .baked_prev
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, r)| r.clone());
        let raster = hit.unwrap_or_else(|| self.bake(node));
        self.baked_new.push((key, raster.clone()));
        raster
    }

    fn bake(&self, node: &CompositeNode) -> Arc<Raster> {
        let mut alone = node.clone();
        alone.visible = true;
        alone.opacity = 1.0;
        alone.blend = BlendMode::Normal;
        alone.blending = Default::default();
        alone.clip_to = None;
        let tree = CompositeTree {
            width: self.width,
            height: self.height,
            space: self.space,
            nodes: vec![alone],
        };
        Arc::new(flatten(&tree, 0))
    }

    fn list(&mut self, nodes: &[CompositeNode], depth: usize) {
        if depth >= MAX_DEPTH {
            self.note(format!("group nesting deeper than {MAX_DEPTH}"));
            return;
        }
        // Clip bases get a compact alpha slot.
        let mut slots = vec![NONE; nodes.len()];
        for node in nodes {
            if let Some(j) = node.clip_to
                && slots[j] == NONE
            {
                if self.alpha_slots < MAX_ALPHA_SLOTS {
                    slots[j] = self.alpha_slots;
                    self.alpha_slots += 1;
                } else {
                    self.note("more than 16 clipping bases".into());
                }
            }
        }
        for (i, node) in nodes.iter().enumerate() {
            if !node.visible {
                continue;
            }
            let clip = match node.clip_to {
                Some(j) if j < i => {
                    if !nodes[j].visible {
                        continue;
                    }
                    slots[j]
                }
                _ => NONE,
            };
            let name = self.name(node.id);
            if node.blending != Default::default() {
                self.note(format!("{name}: advanced blending options ignored"));
            }
            if node.blend == BlendMode::Dissolve {
                self.note(format!("{name}: dissolve drawn as normal"));
            }
            let blend = mode(node.blend);
            let opacity = node.opacity.min(1.0);
            let alpha = slots[i];
            let vector = self.vectors.get(&node.id).cloned();
            match &node.content {
                NodeContent::Pixels { .. } if vector.is_some() && node.mask.is_none() => {
                    let item = VectorItem {
                        node: node.id,
                        kind: vector.unwrap(),
                    };
                    let mergeable = blend == 0 && opacity >= 1.0 && clip == NONE && alpha == NONE;
                    if mergeable && let Some(op) = self.open_run {
                        let Op::Vector { run, .. } = self.ops[op] else {
                            unreachable!()
                        };
                        self.runs[run].push(item);
                        continue;
                    }
                    self.runs.push(vec![item]);
                    self.ops.push(Op::Vector {
                        mode: blend,
                        run: self.runs.len() - 1,
                        opacity,
                        clip,
                        alpha,
                    });
                    self.open_run = mergeable.then_some(self.ops.len() - 1);
                    continue;
                }
                NodeContent::Pixels { raster, placement } => {
                    let (rw, rh) = raster.size();
                    let source = if placement.is_identity()
                        && node.mask.is_none()
                        && rw == self.width
                        && rh == self.height
                    {
                        // A raster layer: the compositor samples these pixels,
                        // so this is where they are rendered if they were
                        // deferred. A Vello-drawn node never reaches here.
                        let s = self.direct_source(name, raster.get().clone(), node.id);
                        if self.paint_node == Some(node.id) {
                            self.paint = Some((node.id, s));
                        }
                        s
                    } else {
                        let baked = self.bake_cached(node, false);
                        self.source(format!("{name} (baked)"), baked)
                    };
                    self.ops.push(Op::Source {
                        mode: blend,
                        source,
                        opacity,
                        clip,
                        alpha,
                    });
                }
                NodeContent::Fill(color) => {
                    let color = if node.mask.is_some() {
                        let baked = self.bake_cached(node, false);
                        let source = self.source(format!("{name} (baked)"), baked);
                        self.ops.push(Op::Source {
                            mode: blend,
                            source,
                            opacity,
                            clip,
                            alpha,
                        });
                        self.open_run = None;
                        continue;
                    } else {
                        *color
                    };
                    self.ops.push(Op::Fill {
                        mode: blend,
                        color,
                        opacity,
                        clip,
                        alpha,
                    });
                }
                NodeContent::Group(children) => {
                    let isolated = node.blend != BlendMode::PassThrough;
                    let mask = node.mask.as_ref().map(|_| {
                        let mut shape = node.clone();
                        shape.content = NodeContent::Fill([1.0; 4]);
                        let baked = self.bake_cached(&shape, true);
                        self.source(format!("{name} mask (baked)"), baked)
                    });
                    self.ops.push(Op::Push { isolated });
                    self.open_run = None;
                    self.list(children, depth + 1);
                    self.ops.push(Op::Pop {
                        isolated,
                        mode: blend,
                        opacity,
                        clip,
                        alpha,
                        mask,
                    });
                }
                NodeContent::StyledGroup { .. } => {
                    self.note(format!("{name}: layer styles skipped"));
                }
                NodeContent::Adjust(_) => {
                    self.note(format!("{name}: adjustment layer skipped"));
                }
            }
            self.open_run = None;
        }
    }
}

/// Everything about a composite node that decides how it compiles, with
/// its pixel content identified separately.
///
/// Two documents with equal signatures compile to the same program, so a
/// difference confined to `content` means only pixels moved and the engine
/// can swap those rasters instead of rebuilding.
#[derive(Clone, PartialEq)]
pub struct NodeSig {
    pub node: NodeId,
    /// `Arc` address of the pixel content, or 0.
    pub content: usize,
    shape: u64,
    mask: usize,
    placement: Option<emulsion_raster::composite::Placement>,
    opacity: u32,
    clip_to: Option<usize>,
}

impl Canvas {
    /// Compile `doc` and upload its tiles into a new atlas with room for
    /// `headroom` more. `paint_node` names the raster node strokes modify.
    /// Without `vello`, vector nodes composite from their CPU caches.
    pub fn compile(
        doc: &Document,
        gpu: &Arc<crate::gpu::Gpu>,
        paint_node: Option<NodeId>,
        headroom: u32,
        vello: bool,
    ) -> anyhow::Result<(Self, Atlas)> {
        let device = &gpu.device;
        let tree = doc.composite_tree();
        let mut compiler = Compiler {
            vectors: if vello {
                vector_nodes(doc)
            } else {
                HashMap::new()
            },
            names: doc.nodes.iter().map(|n| (n.id, n.name.clone())).collect(),
            width: doc.width,
            height: doc.height,
            space: doc.blend_space,
            sources: Vec::new(),
            ops: Vec::new(),
            runs: Vec::new(),
            open_run: None,
            unsupported: Vec::new(),
            alpha_slots: 0,
            paint: None,
            paint_node,
            baked_prev: Vec::new(),
            baked_new: Vec::new(),
            _doc: doc,
        };
        compiler.list(&tree.nodes, 0);
        let unique: std::collections::HashSet<usize> = compiler
            .sources
            .iter()
            .flat_map(|(_, r, _)| r.base_tiles().map(|(_, t)| t.as_ptr() as usize))
            .collect();
        let filled: u32 = compiler
            .sources
            .iter()
            .filter(|(_, r, _)| r.fill() != [0; 4])
            .map(|(_, r, _)| {
                let (x, y) = r.tiles_at(0);
                (x * y) as u32
            })
            .sum();
        let mut atlas = Atlas::new(gpu.clone(), unique.len() as u32 + filled + headroom);
        let canvas = Self::assemble(compiler, doc, device, &mut atlas)?;
        Ok((canvas, atlas))
    }

    /// Rebuild the program and tile tables for a changed document, reusing the
    /// existing atlas.
    ///
    /// The atlas keys tiles by `Arc` identity, so every tile the edit left
    /// alone is re-acquired from its existing slot and never re-uploaded. New
    /// tiles are acquired before the old slots are released, so a tile present
    /// in both documents never drops to zero references in between and never
    /// makes a round trip to the GPU.
    ///
    /// Returns an error if the atlas cannot fit the new document, in which case
    /// the caller should fall back to [`Self::compile`] and a fresh atlas; the
    /// atlas may hold slots from the abandoned attempt.
    pub fn recompile(
        &mut self,
        doc: &Document,
        gpu: &Arc<crate::gpu::Gpu>,
        atlas: &mut Atlas,
        paint_node: Option<NodeId>,
        vello: bool,
    ) -> anyhow::Result<()> {
        let tree = doc.composite_tree();
        let mut compiler = Compiler {
            vectors: if vello {
                vector_nodes(doc)
            } else {
                HashMap::new()
            },
            names: doc.nodes.iter().map(|n| (n.id, n.name.clone())).collect(),
            width: doc.width,
            height: doc.height,
            space: doc.blend_space,
            sources: Vec::new(),
            ops: Vec::new(),
            runs: Vec::new(),
            open_run: None,
            unsupported: Vec::new(),
            alpha_slots: 0,
            paint: None,
            paint_node,
            baked_prev: std::mem::take(&mut self.baked),
            baked_new: Vec::new(),
            _doc: doc,
        };
        compiler.list(&tree.nodes, 0);
        let next = Self::assemble(compiler, doc, &gpu.device, atlas)?;
        // Only now: anything the new document still uses has already been
        // re-acquired, so releasing cannot evict a live tile.
        for source in &self.sources {
            for slot in source.slots.iter().flatten() {
                atlas.release(*slot);
            }
        }
        // Tell the composite cache only what actually changed. A stroke
        // commits on every frame, so blanket-invalidating here would recomposite
        // the whole view each time and undo the point of the cache.
        let mut dirty = Vec::new();
        let structural = next.ops != self.ops
            || next.sources.len() != self.sources.len()
            || next.space != self.space;
        if structural {
            dirty.push(IRect::new(0, 0, doc.width as i32, doc.height as i32));
        } else {
            let t = TILE as i32;
            for (before, after) in self.sources.iter().zip(&next.sources) {
                if before.tiles_x != after.tiles_x || before.tiles_y != after.tiles_y {
                    dirty.push(IRect::new(0, 0, doc.width as i32, doc.height as i32));
                    break;
                }
                for (i, (b, a)) in before.slots.iter().zip(&after.slots).enumerate() {
                    if b != a {
                        let (x, y) = (i as u32 % after.tiles_x, i as u32 / after.tiles_x);
                        dirty.push(IRect::new(x as i32 * t, y as i32 * t, t, t));
                    }
                }
            }
        }
        *self = next;
        self.dirty.extend(dirty);
        Ok(())
    }

    /// Build the sources, tile tables and program against `atlas`.
    fn assemble(
        compiler: Compiler<'_>,
        doc: &Document,
        device: &wgpu::Device,
        atlas: &mut Atlas,
    ) -> anyhow::Result<Self> {
        let tiles_x = doc.width.div_ceil(TILE);
        let tiles_y = doc.height.div_ceil(TILE);
        let per = (tiles_x * tiles_y) as usize;
        let mut sources = Vec::new();
        let mut table = Vec::new();
        for (name, raster, node) in compiler.sources {
            // Missing tiles read as the fill; one shared tile materialises them.
            let fill_tile: Option<Arc<[[u16; 4]]>> =
                (raster.fill() != [0; 4]).then(|| atlas.fill_tile(raster.fill()));
            let mut slots = Vec::with_capacity(per);
            for ty in 0..tiles_y {
                for tx in 0..tiles_x {
                    let c = TileCoord::new(tx as i32, ty as i32);
                    let slot = match raster.base_tile(c) {
                        Some(tile) => Some(
                            atlas
                                .acquire(tile)
                                .ok_or_else(|| anyhow::anyhow!("tile atlas full"))?,
                        ),
                        None => match &fill_tile {
                            Some(tile) => Some(
                                atlas
                                    .acquire(tile)
                                    .ok_or_else(|| anyhow::anyhow!("tile atlas full"))?,
                            ),
                            None => None,
                        },
                    };
                    slots.push(slot);
                }
            }
            let table_offset = table.len() as u32;
            table.extend(slots.iter().map(|s| s.unwrap_or(NONE)));
            sources.push(Source {
                name,
                node,
                raster,
                tiles_x,
                tiles_y,
                table_offset,
                slots,
            });
        }
        if table.is_empty() {
            table.push(NONE);
        }
        let tables = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("tile tables"),
                contents: bytemuck::cast_slice(&table),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            },
        );
        Ok(Self {
            width: doc.width,
            height: doc.height,
            space: doc.blend_space,
            sources,
            ops: compiler.ops,
            runs: compiler.runs,
            unsupported: compiler.unsupported,
            paint: compiler.paint.map(|(_, s)| s),
            table,
            tables,
            dirty: Vec::new(),
            baked: compiler.baked_new,
        })
    }

    /// The longest run of leading ops the composite cache can hold: it
    /// contains no Vello run, ends outside any group, and no later op clips
    /// to an alpha slot it sets.
    /// Signature of a document's composite tree, in compile order.
    pub fn signature(doc: &Document) -> Vec<NodeSig> {
        fn walk(nodes: &[CompositeNode], out: &mut Vec<NodeSig>) {
            for node in nodes {
                let (content, placement) = match &node.content {
                    NodeContent::Pixels { raster, placement } => (raster.id(), Some(*placement)),
                    _ => (0, None),
                };
                // Everything that changes the emitted ops, folded together.
                let shape = {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    use std::hash::{Hash, Hasher};
                    std::mem::discriminant(&node.content).hash(&mut h);
                    node.visible.hash(&mut h);
                    format!("{:?}", node.blend).hash(&mut h);
                    format!("{:?}", node.blending).hash(&mut h);
                    if let NodeContent::Fill(c) = &node.content {
                        c.map(f32::to_bits).hash(&mut h);
                    }
                    h.finish()
                };
                out.push(NodeSig {
                    node: node.id,
                    content,
                    shape,
                    mask: node
                        .mask
                        .as_ref()
                        .map_or(0, |m| Arc::as_ptr(m) as *const u8 as usize),
                    placement,
                    opacity: node.opacity.to_bits(),
                    clip_to: node.clip_to,
                });
                if let NodeContent::Group(children) = &node.content {
                    walk(children, out);
                }
            }
        }
        let tree = doc.composite_tree();
        let mut out = Vec::new();
        walk(&tree.nodes, &mut out);
        out
    }

    /// Nodes whose pixels changed, when that is the *only* difference between
    /// two signatures. `None` means the program itself must be rebuilt.
    pub fn pixels_only_change(before: &[NodeSig], after: &[NodeSig]) -> Option<Vec<NodeId>> {
        if before.len() != after.len() {
            return None;
        }
        let mut changed = Vec::new();
        for (a, b) in before.iter().zip(after) {
            let same_frame = NodeSig {
                content: b.content,
                ..a.clone()
            } == *b;
            if !same_frame {
                return None;
            }
            if a.content != b.content {
                changed.push(b.node);
            }
        }
        Some(changed)
    }

    /// Identity of the vector content, for deciding whether the Vello layer
    /// has to be re-encoded.
    ///
    /// Geometry and text are compared by `Arc` address, which an edit always
    /// changes because the document stores them behind `Arc`; style is stored
    /// by value and compared directly. Re-encoding every object costs about
    /// 20 ms on a 4K document, so a raster-only edit must not pay it.
    pub fn vector_signature(&self) -> Vec<(NodeId, usize, Option<PathStyle>)> {
        self.runs
            .iter()
            .flatten()
            .map(|item| match &item.kind {
                VectorKind::Path { path, style } => (
                    item.node,
                    Arc::as_ptr(path) as *const u8 as usize,
                    Some(*style),
                ),
                VectorKind::Text { spec } => {
                    (item.node, Arc::as_ptr(spec) as *const u8 as usize, None)
                }
            })
            .collect()
    }

    pub fn cacheable_prefix(&self) -> usize {
        let first_vector = self
            .ops
            .iter()
            .position(|op| matches!(op, Op::Vector { .. }))
            .unwrap_or(self.ops.len());
        let clip_of = |op: &Op| match *op {
            Op::Source { clip, .. }
            | Op::Fill { clip, .. }
            | Op::Vector { clip, .. }
            | Op::Pop { clip, .. } => clip,
            Op::Push { .. } => NONE,
        };
        let alpha_of = |op: &Op| match *op {
            Op::Source { alpha, .. }
            | Op::Fill { alpha, .. }
            | Op::Vector { alpha, .. }
            | Op::Pop { alpha, .. } => alpha,
            Op::Push { .. } => NONE,
        };
        let mut depth = 0i32;
        let mut best = 0;
        for end in 0..=first_vector {
            if depth == 0 {
                let set: Vec<u32> = self.ops[..end]
                    .iter()
                    .map(alpha_of)
                    .filter(|a| *a != NONE)
                    .collect();
                if self.ops[end..].iter().all(|op| !set.contains(&clip_of(op))) {
                    best = end;
                }
            }
            if let Some(op) = self.ops.get(end) {
                match op {
                    Op::Push { .. } => depth += 1,
                    Op::Pop { .. } => depth -= 1,
                    _ => {}
                }
            }
        }
        best
    }

    /// Record that GPU painting changed tile `c`.
    pub fn mark_tile(&mut self, c: TileCoord) {
        let t = TILE as i32;
        self.dirty.push(IRect::new(c.x * t, c.y * t, t, t));
    }

    /// Program words for the composite shader.
    pub fn program(&self) -> Vec<u32> {
        let mut words = vec![0u32; HEADER_WORDS];
        words[0] = self.ops.len() as u32;
        words[1] = u32::from(self.space == BlendSpace::Srgb);
        for op in &self.ops {
            let mut w = [0u32; OP_WORDS];
            let source = |w: &mut [u32; OP_WORDS], s: &Source| {
                w[5] = s.table_offset;
                w[6] = s.tiles_x;
                w[7] = s.tiles_y;
            };
            match *op {
                Op::Source {
                    mode,
                    source: s,
                    opacity,
                    clip,
                    alpha,
                } => {
                    w[..5].copy_from_slice(&[0, mode, alpha, clip, opacity.to_bits()]);
                    source(&mut w, &self.sources[s]);
                }
                Op::Fill {
                    mode,
                    color,
                    opacity,
                    clip,
                    alpha,
                } => {
                    w[..5].copy_from_slice(&[5, mode, alpha, clip, opacity.to_bits()]);
                    for (i, c) in color.iter().enumerate() {
                        w[8 + i] = c.to_bits();
                    }
                }
                Op::Vector {
                    mode,
                    run,
                    opacity,
                    clip,
                    alpha,
                } => {
                    w[..5].copy_from_slice(&[6, mode, alpha, clip, opacity.to_bits()]);
                    w[5] = run as u32;
                }
                Op::Push { isolated } => w[0] = if isolated { 1 } else { 2 },
                Op::Pop {
                    isolated,
                    mode,
                    opacity,
                    clip,
                    alpha,
                    mask,
                } => {
                    w[..5].copy_from_slice(&[
                        if isolated { 3 } else { 4 },
                        mode,
                        alpha,
                        clip,
                        opacity.to_bits(),
                    ]);
                    w[8] = NONE;
                    if let Some(m) = mask {
                        source(&mut w, &self.sources[m]);
                        w[8] = 1;
                    }
                }
            }
            words.extend_from_slice(&w);
        }
        words
    }

    /// Point `source`'s tiles at `raster`, uploading only tiles whose
    /// identity changed. Returns the number of tiles uploaded.
    pub fn replace_raster(
        &mut self,
        queue: &wgpu::Queue,
        atlas: &mut Atlas,
        source: usize,
        raster: Arc<Raster>,
        only: Option<&[TileCoord]>,
    ) -> anyhow::Result<usize> {
        let s = &mut self.sources[source];
        let old = std::mem::replace(&mut s.raster, raster);
        let coords: Vec<TileCoord> = match only {
            Some(c) => c.to_vec(),
            None => (0..s.tiles_y as i32)
                .flat_map(|y| (0..s.tiles_x as i32).map(move |x| TileCoord::new(x, y)))
                .collect(),
        };
        let mut changed = 0;
        for c in coords {
            let Some(i) = s.index(c) else { continue };
            let new = s.raster.base_tile(c);
            let same = match (old.base_tile(c), new) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            let slot = match new {
                Some(tile) => Some(
                    atlas
                        .acquire(tile)
                        .ok_or_else(|| anyhow::anyhow!("tile atlas full"))?,
                ),
                None => None,
            };
            if let Some(old_slot) = std::mem::replace(&mut s.slots[i], slot) {
                atlas.release(old_slot);
            }
            let entry = s.table_offset as usize + i;
            self.table[entry] = slot.unwrap_or(NONE);
            queue.write_buffer(
                &self.tables,
                entry as u64 * 4,
                bytemuck::bytes_of(&self.table[entry]),
            );
            let t = TILE as i32;
            self.dirty.push(IRect::new(c.x * t, c.y * t, t, t));
            changed += 1;
        }
        Ok(changed)
    }

    /// Give `source`'s tile `c` a slot this canvas owns alone, for GPU
    /// painting. New tiles start transparent.
    pub fn own_tile(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        atlas: &mut Atlas,
        source: usize,
        c: TileCoord,
    ) -> anyhow::Result<Option<(Slot, bool)>> {
        let s = &mut self.sources[source];
        let Some(i) = s.index(c) else {
            return Ok(None);
        };
        let (slot, fresh) = match s.slots[i] {
            Some(slot) if !atlas.is_shared(slot) => {
                atlas.detach(slot);
                (slot, false)
            }
            Some(shared) => {
                let copy = atlas
                    .duplicate(encoder, shared)
                    .ok_or_else(|| anyhow::anyhow!("tile atlas full"))?;
                atlas.release(shared);
                (copy, false)
            }
            None => (
                atlas
                    .alloc()
                    .ok_or_else(|| anyhow::anyhow!("tile atlas full"))?,
                true,
            ),
        };
        if s.slots[i] != Some(slot) {
            s.slots[i] = Some(slot);
            let entry = s.table_offset as usize + i;
            self.table[entry] = slot;
            queue.write_buffer(&self.tables, entry as u64 * 4, bytemuck::bytes_of(&slot));
        }
        Ok(Some((slot, fresh)))
    }

    /// Take `raster` as `source`'s CPU pixels when the atlas already holds
    /// them (GPU painting read back), without uploading.
    pub fn adopt_raster(
        &mut self,
        atlas: &mut Atlas,
        source: usize,
        raster: Arc<Raster>,
        painted: &[(TileCoord, Slot)],
    ) {
        let s = &mut self.sources[source];
        s.raster = raster;
        for (c, slot) in painted {
            if let (Some(i), Some(tile)) = (s.index(*c), s.raster.base_tile(*c))
                && s.slots[i] == Some(*slot)
            {
                atlas.attach(*slot, tile);
            }
        }
    }

    pub fn vector_count(&self) -> usize {
        self.runs.iter().map(Vec::len).sum()
    }
}
