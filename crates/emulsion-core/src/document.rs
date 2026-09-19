//! The document: canvas size plus a flat, bottom-to-top list of nodes.
//!
//! Groups are stored as a contiguous block: a group's descendants sit
//! immediately below it in the list, so the list reads like the rendered
//! stack and the tree can be rebuilt from parent pointers alone.

use crate::node::{Node, NodeId, NodeKind};
use emulsion_raster::blend::BlendSpace;
use emulsion_raster::color;
use emulsion_raster::composite::{CompositeNode, CompositeTree, NodeContent};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DocumentError {
    #[error("canvas size {0}×{1} is outside 1–30000 px per side or 400 MP total")]
    CanvasSize(u32, u32),
    #[error("duplicate node id {0}")]
    DuplicateId(NodeId),
    #[error("node {0} has missing parent {1}")]
    MissingParent(NodeId, NodeId),
    #[error("node {0} has a parent {1} that is not a group")]
    ParentNotGroup(NodeId, NodeId),
    #[error("group {0} is not contiguous with its descendants")]
    NotContiguous(NodeId),
    #[error("groups nest deeper than {0}")]
    TooDeep(usize),
    #[error("node {0} clips to {1}, which is not a sibling below it")]
    BadClip(NodeId, NodeId),
    #[error("node {0} has a non-finite or out-of-range value: {1}")]
    BadValue(NodeId, &'static str),
    #[error("guides must be finite and at most 500")]
    BadGuides,
    #[error("more than {0} nodes")]
    TooManyNodes(usize),
}

pub const MAX_SIDE: u32 = 30_000;
pub const MAX_PIXELS: u64 = 400_000_000;
pub const MAX_DEPTH: usize = 64;
pub const MAX_NODES: usize = 10_000;

/// A ruler guide: a vertical line at x = pos, or a horizontal one at y = pos.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Guide {
    pub vertical: bool,
    pub pos: f64,
}

pub const MAX_GUIDES: usize = 500;

#[derive(Clone, Debug)]
pub struct Document {
    pub width: u32,
    pub height: u32,
    /// Pixels per inch, recorded for export.
    pub resolution: f32,
    /// Bit depth of the source the document came from (8 or 16), for the
    /// title bar and export defaults. Storage is always 16-bit linear.
    pub source_depth: u8,
    pub blend_space: BlendSpace,
    /// Bottom to top.
    pub nodes: Vec<Node>,
    pub next_id: NodeId,
    /// Document-space coverage; `None` means no selection (everything).
    pub selection: Option<Arc<emulsion_raster::Mask>>,
    /// Ruler guides, for snapping and alignment. Not rendered into pixels.
    pub guides: Vec<Guide>,
}

impl PartialEq for Document {
    fn eq(&self, o: &Self) -> bool {
        self.width == o.width
            && self.height == o.height
            && self.resolution == o.resolution
            && self.blend_space == o.blend_space
            && self.nodes == o.nodes
            && self.guides == o.guides
            && match (&self.selection, &o.selection) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

/// One row of the node panel, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelRow {
    pub id: NodeId,
    pub depth: usize,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            resolution: 72.0,
            source_depth: 8,
            blend_space: BlendSpace::Linear,
            nodes: Vec::new(),
            next_id: 1,
            selection: None,
            guides: Vec::new(),
        }
    }

    pub fn alloc_id(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn index_of(&self, id: NodeId) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Direct children of `parent` (`None` = roots), bottom to top.
    pub fn children(&self, parent: Option<NodeId>) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|n| n.parent == parent)
            .map(|n| n.id)
            .collect()
    }

    /// `id` and everything under it.
    pub fn subtree(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let p = out[i];
            out.extend(
                self.nodes
                    .iter()
                    .filter(|n| n.parent == Some(p))
                    .map(|n| n.id),
            );
            i += 1;
        }
        out
    }

    pub fn is_ancestor(&self, ancestor: NodeId, mut of: NodeId) -> bool {
        while let Some(p) = self.node(of).and_then(|n| n.parent) {
            if p == ancestor {
                return true;
            }
            of = p;
        }
        false
    }

    pub fn depth(&self, id: NodeId) -> usize {
        let mut d = 0;
        let mut cur = id;
        while let Some(p) = self.node(cur).and_then(|n| n.parent) {
            d += 1;
            cur = p;
        }
        d
    }

    /// Rebuild `nodes` in canonical order (each group directly above its
    /// descendants) from parent pointers and current sibling order.
    pub fn normalize(&mut self) {
        let by_id: HashMap<NodeId, Node> = self.nodes.iter().map(|n| (n.id, n.clone())).collect();
        let mut kids: HashMap<Option<NodeId>, Vec<NodeId>> = HashMap::new();
        for n in &self.nodes {
            kids.entry(n.parent).or_default().push(n.id);
        }
        let mut out = Vec::with_capacity(self.nodes.len());
        fn emit(
            id: NodeId,
            kids: &HashMap<Option<NodeId>, Vec<NodeId>>,
            by_id: &HashMap<NodeId, Node>,
            out: &mut Vec<Node>,
        ) {
            if let Some(ch) = kids.get(&Some(id)) {
                for c in ch {
                    emit(*c, kids, by_id, out);
                }
            }
            out.push(by_id[&id].clone());
        }
        if let Some(roots) = kids.get(&None) {
            for r in roots {
                emit(*r, &kids, &by_id, &mut out);
            }
        }
        self.nodes = out;
    }

    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.guides.len() > MAX_GUIDES
            || self
                .guides
                .iter()
                .any(|g| !g.pos.is_finite() || g.pos.abs() > 1e6)
        {
            return Err(DocumentError::BadGuides);
        }
        if self.width == 0
            || self.height == 0
            || self.width > MAX_SIDE
            || self.height > MAX_SIDE
            || self.width as u64 * self.height as u64 > MAX_PIXELS
        {
            return Err(DocumentError::CanvasSize(self.width, self.height));
        }
        if self.nodes.len() > MAX_NODES {
            return Err(DocumentError::TooManyNodes(MAX_NODES));
        }
        let mut seen = HashSet::new();
        for n in &self.nodes {
            if !seen.insert(n.id) {
                return Err(DocumentError::DuplicateId(n.id));
            }
        }
        for n in &self.nodes {
            if let Some(p) = n.parent {
                match self.node(p) {
                    None => return Err(DocumentError::MissingParent(n.id, p)),
                    Some(pn) if !pn.is_group() => {
                        return Err(DocumentError::ParentNotGroup(n.id, p));
                    }
                    _ => {}
                }
            }
            if !(0.0..=1.0).contains(&n.opacity) || !n.opacity.is_finite() {
                return Err(DocumentError::BadValue(n.id, "opacity"));
            }
            if let NodeKind::Raster { placement, .. } = &n.kind {
                let p = placement;
                let finite = [p.x, p.y, p.scale_x, p.scale_y, p.rotation]
                    .iter()
                    .all(|v| v.is_finite());
                if !finite || p.scale_x.abs() < 1e-6 || p.scale_y.abs() < 1e-6 {
                    return Err(DocumentError::BadValue(n.id, "placement"));
                }
            }
            if let NodeKind::Adjust(a) = &n.kind
                && a.params().iter().any(|s| !s.value.is_finite())
            {
                return Err(DocumentError::BadValue(n.id, "adjustment"));
            }
        }
        for n in &self.nodes {
            if self.depth(n.id) > MAX_DEPTH {
                return Err(DocumentError::TooDeep(MAX_DEPTH));
            }
        }
        // Contiguity: the nodes just below each group are exactly its descendants.
        for (i, n) in self.nodes.iter().enumerate() {
            if n.is_group() {
                let size = self.subtree(n.id).len() - 1;
                if size > i {
                    return Err(DocumentError::NotContiguous(n.id));
                }
                for m in &self.nodes[i - size..i] {
                    if !self.is_ancestor(n.id, m.id) {
                        return Err(DocumentError::NotContiguous(n.id));
                    }
                }
            }
        }
        for n in &self.nodes {
            if let Some(c) = n.clip_to {
                let siblings = self.children(n.parent);
                let me = siblings.iter().position(|s| *s == n.id);
                let base = siblings.iter().position(|s| *s == c);
                match (me, base) {
                    (Some(m), Some(b)) if b < m => {}
                    _ => return Err(DocumentError::BadClip(n.id, c)),
                }
            }
        }
        Ok(())
    }

    /// Node panel rows, top to bottom, skipping children of collapsed groups.
    pub fn panel_rows(&self) -> Vec<PanelRow> {
        let mut out = Vec::new();
        fn walk(doc: &Document, parent: Option<NodeId>, depth: usize, out: &mut Vec<PanelRow>) {
            for id in doc.children(parent).into_iter().rev() {
                out.push(PanelRow { id, depth });
                if let Some(NodeKind::Group { collapsed: false }) = doc.node(id).map(|n| &n.kind) {
                    walk(doc, Some(id), depth + 1, out);
                }
            }
        }
        walk(self, None, 0, &mut out);
        out
    }

    /// Build the render description.
    pub fn composite_tree(&self) -> CompositeTree {
        fn build(doc: &Document, parent: Option<NodeId>) -> Vec<CompositeNode> {
            let ids = doc.children(parent);
            ids.iter()
                .map(|id| {
                    let n = doc.node(*id).expect("child exists");
                    let content = match &n.kind {
                        NodeKind::Raster { raster, placement } => NodeContent::Pixels {
                            raster: raster.clone(),
                            placement: *placement,
                        },
                        NodeKind::Group { .. } => NodeContent::Group(build(doc, Some(n.id))),
                        NodeKind::Adjust(a) => NodeContent::Adjust(Arc::new(a.prepare())),
                        NodeKind::Fill { rgba } => {
                            NodeContent::Fill(color::srgba8_to_premul(*rgba))
                        }
                        NodeKind::Path { cache, .. } => NodeContent::Pixels {
                            raster: cache.clone(),
                            placement: emulsion_raster::Placement::default(),
                        },
                        NodeKind::Smart {
                            source,
                            placement,
                            cache,
                            offset,
                            ..
                        } => NodeContent::Pixels {
                            raster: cache.clone(),
                            placement: crate::smart::cache_placement(
                                placement,
                                (source.width(), source.height()),
                                (cache.width(), cache.height()),
                                *offset,
                            ),
                        },
                    };
                    (
                        n,
                        CompositeNode {
                            id: n.id,
                            visible: n.visible,
                            opacity: n.opacity,
                            blend: n.blend,
                            mask: if n.mask_enabled { n.mask.clone() } else { None },
                            clip_to: None,
                            content,
                        },
                    )
                })
                .collect::<Vec<_>>()
                .into_iter()
                .flat_map(|(n, node)| {
                    // Layer styles become pixel layers beside the node.
                    let mut out: Vec<(Option<NodeId>, CompositeNode)> = Vec::with_capacity(3);
                    let fx = if n.visible {
                        crate::styles::render(doc, n)
                    } else {
                        None
                    };
                    if let Some(r) = &fx
                        && let Some((raster, rect)) = &r.below
                    {
                        out.push((
                            None,
                            crate::styles::effect_node(n.id ^ (1 << 62), raster.clone(), *rect, n),
                        ));
                    }
                    out.push((Some(n.id), node));
                    if let Some(r) = &fx
                        && let Some((raster, rect)) = &r.above
                    {
                        out.push((
                            None,
                            crate::styles::effect_node(n.id ^ (1 << 63), raster.clone(), *rect, n),
                        ));
                    }
                    out
                })
                .collect::<Vec<_>>()
                .into_iter()
                .enumerate()
                .map(|(i, (owner, mut node))| {
                    // Clip targets are positions in this list, which effects shifted.
                    (i, owner, node.clip_to.take(), node)
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(_, owner, _, mut node)| {
                    if let Some(id) = owner
                        && let Some(c) = doc.node(id).and_then(|n| n.clip_to)
                    {
                        node.clip_to = positions_of(doc, parent, c);
                    }
                    node
                })
                .collect()
        }
        /// Position of node `c`'s own entry in the composite list of `parent`.
        fn positions_of(doc: &Document, parent: Option<NodeId>, c: NodeId) -> Option<usize> {
            let mut i = 0;
            for id in doc.children(parent) {
                let n = doc.node(id).expect("child exists");
                let fx = if n.visible {
                    crate::styles::render(doc, n)
                } else {
                    None
                };
                if let Some(r) = &fx
                    && r.below.is_some()
                {
                    i += 1;
                }
                if id == c {
                    return Some(i);
                }
                i += 1;
                if let Some(r) = &fx
                    && r.above.is_some()
                {
                    i += 1;
                }
            }
            None
        }
        CompositeTree {
            width: self.width,
            height: self.height,
            space: self.blend_space,
            nodes: build(self, None),
        }
    }

    /// Distinct pixel buffers referenced by this document, for memory
    /// accounting.
    /// A node's visible coverage as a document-space selection: its pixels'
    /// alpha (through its mask) for pixel and fill nodes, or its mask for
    /// adjustments and groups. None when the node covers nothing.
    pub fn node_coverage(&self, id: NodeId) -> Option<emulsion_raster::Mask> {
        let n = self.node(id)?;
        if !matches!(
            n.kind,
            NodeKind::Raster { .. }
                | NodeKind::Fill { .. }
                | NodeKind::Path { .. }
                | NodeKind::Smart { .. }
        ) {
            // Mask-only nodes: the mask is already in document space.
            return n.mask.as_ref().map(|m| (**m).clone());
        }
        let mut solo = Document::new(self.width, self.height);
        let mut node = n.clone();
        node.parent = None;
        node.clip_to = None;
        node.visible = true;
        node.opacity = 1.0;
        node.blend = emulsion_raster::BlendMode::Normal;
        solo.nodes.push(node);
        let full = emulsion_raster::composite::flatten(&solo.composite_tree(), 0);
        let region = full.tile_bounds();
        let px = full.read_rect(region);
        let alpha: Vec<u8> = px
            .iter()
            .map(|p| (color::u16_to_f(p[3]) * 255.0).round() as u8)
            .collect();
        let m = emulsion_raster::Mask::empty(self.width, self.height, 0).write_rect(region, &alpha);
        (!emulsion_raster::select::bounds(&m).is_empty()).then_some(m)
    }

    pub fn buffers(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for n in &self.nodes {
            if let NodeKind::Raster { raster, .. } = &n.kind {
                out.push((
                    Arc::as_ptr(raster) as usize,
                    raster.tile_count() * 256 * 256 * 8,
                ));
            }
            if let NodeKind::Path { cache, .. } = &n.kind {
                out.push((
                    Arc::as_ptr(cache) as usize,
                    cache.tile_count() * 256 * 256 * 8,
                ));
            }
            if let NodeKind::Smart { source, cache, .. } = &n.kind {
                out.push((
                    Arc::as_ptr(source) as usize,
                    source.tile_count() * 256 * 256 * 8,
                ));
                out.push((
                    Arc::as_ptr(cache) as usize,
                    cache.tile_count() * 256 * 256 * 8,
                ));
            }
            if let Some(m) = &n.mask {
                out.push((Arc::as_ptr(m) as usize, m.tile_count() * 256 * 256));
            }
        }
        out
    }
}
