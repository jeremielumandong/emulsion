//! The document: canvas size plus a flat, bottom-to-top list of nodes.
//!
//! Groups are stored as a contiguous block: a group's descendants sit
//! immediately below it in the list, so the list reads like the rendered
//! stack and the tree can be rebuilt from parent pointers alone.

use crate::node::{Node, NodeId, NodeKind};
use emulsion_raster::Mask;
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
    pub global_light: crate::style_options::GlobalLight,
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
    /// What the camera recorded, when the document came from a photograph.
    pub info: Option<ImageInfo>,
}

/// Camera metadata carried from the source file (EXIF), for the Info
/// panel, lens profiles and the assistant.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ImageInfo {
    pub make: String,
    pub model: String,
    pub lens: String,
    /// Focal length in mm, as shot.
    pub focal_mm: f32,
    /// Full-frame equivalent focal length, when the file says.
    pub focal_35mm: f32,
    pub f_number: f32,
    /// Seconds.
    pub exposure_s: f32,
    pub iso: u32,
    pub taken: String,
    pub software: String,
}

impl ImageInfo {
    /// "Sony ILCE-7M3 · FE 24-70mm F2.8 GM · 47 mm · f/2.8 · 1/250 s · ISO 400".
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let cam = format!("{} {}", self.make, self.model).trim().to_string();
        if !cam.is_empty() {
            parts.push(cam);
        }
        if !self.lens.is_empty() {
            parts.push(self.lens.clone());
        }
        if self.focal_mm > 0.0 {
            parts.push(format!("{:.0} mm", self.focal_mm));
        }
        if self.f_number > 0.0 {
            parts.push(format!("f/{:.1}", self.f_number));
        }
        if self.exposure_s > 0.0 {
            parts.push(if self.exposure_s < 1.0 {
                format!("1/{:.0} s", 1.0 / self.exposure_s)
            } else {
                format!("{:.1} s", self.exposure_s)
            });
        }
        if self.iso > 0 {
            parts.push(format!("ISO {}", self.iso));
        }
        parts.join(" · ")
    }
}

impl PartialEq for Document {
    fn eq(&self, o: &Self) -> bool {
        self.width == o.width
            && self.height == o.height
            && self.resolution == o.resolution
            && self.global_light == o.global_light
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
            global_light: Default::default(),
            source_depth: 8,
            blend_space: BlendSpace::Linear,
            nodes: Vec::new(),
            next_id: 1,
            selection: None,
            guides: Vec::new(),
            info: None,
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

    /// The first lock protecting this node, including its containing groups.
    /// UI tools can use this before starting a gesture; commands enforce it too.
    pub fn locked_ancestor(&self, id: NodeId) -> Option<NodeId> {
        let mut current = Some(id);
        for _ in 0..=self.nodes.len() {
            let node = self.node(current?)?;
            if node.locked {
                return Some(node.id);
            }
            current = node.parent;
        }
        None
    }

    /// Smart masks live in source pixel coordinates, like ordinary raster
    /// masks. An expanded filter cache needs the same mask shifted by its
    /// source offset before the compositor samples it through cache placement.
    pub fn composite_mask(node: &Node) -> Option<Arc<Mask>> {
        crate::composite_mask_cache::composite_mask(node)
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

    /// Effective granular locks, including parent groups.
    pub fn layer_locks(&self, id: NodeId) -> crate::node::LayerLocks {
        let mut locks = crate::node::LayerLocks::default();
        let mut current = Some(id);
        for _ in 0..=self.nodes.len() {
            let Some(node) = current.and_then(|id| self.node(id)) else {
                break;
            };
            locks.transparency |= node.locks.transparency;
            locks.pixels |= node.locks.pixels;
            locks.position |= node.locks.position;
            current = node.parent;
        }
        locks
    }

    pub fn validate(&self) -> Result<(), DocumentError> {
        if !self.global_light.valid() {
            return Err(DocumentError::BadValue(0, "global light"));
        }
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
            if n.style_options.len() > n.styles.len() || n.style_options.iter().any(|o| !o.valid())
            {
                return Err(DocumentError::BadValue(n.id, "style options"));
            }
            let mask_affine = glam::DAffine2::from_cols_array(&n.mask_transform);
            if !n.mask_transform.iter().all(|v| v.is_finite())
                || mask_affine.matrix2.determinant().abs() < 1e-12
            {
                return Err(DocumentError::BadValue(n.id, "mask transform"));
            }
            if !n.blending.valid() {
                return Err(DocumentError::BadValue(n.id, "blending options"));
            }
            if !(0.0..=1.0).contains(&n.opacity) || !n.opacity.is_finite() {
                return Err(DocumentError::BadValue(n.id, "opacity"));
            }
            if let NodeKind::Raster { placement, .. } | NodeKind::Smart { placement, .. } = &n.kind
            {
                let p = placement;
                let finite = [p.x, p.y, p.scale_x, p.scale_y, p.rotation]
                    .iter()
                    .all(|v| v.is_finite());
                if !finite || p.scale_x.abs() < 1e-6 || p.scale_y.abs() < 1e-6 {
                    return Err(DocumentError::BadValue(n.id, "placement"));
                }
            }
            if let Some(mask) = &n.mask {
                let expected = match &n.kind {
                    NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
                    NodeKind::Smart { source, .. } => (source.width(), source.height()),
                    _ => (self.width, self.height),
                };
                if (mask.width(), mask.height()) != expected {
                    return Err(DocumentError::BadValue(n.id, "mask size"));
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
                        NodeKind::Path { cache, .. } | NodeKind::Text { cache, .. } => {
                            NodeContent::Pixels {
                                raster: cache.clone(),
                                placement: emulsion_raster::Placement::default(),
                            }
                        }
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
                            blending: n.blending,
                            mask: Document::composite_mask(n),
                            clip_to: None,
                            content,
                        },
                    )
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(n, mut node)| {
                    // Composite the complete styled appearance once against the real
                    // backdrop. Fill affects content only; layer opacity, channels,
                    // blend mode and tonal gates affect content and effects together.
                    if let Some(fx) = n.visible.then(|| crate::styles::render(doc, n)).flatten() {
                        let mut clip_source = node.clone();
                        clip_source.opacity = 1.0;
                        clip_source.blend = emulsion_raster::BlendMode::Normal;
                        clip_source.blending = Default::default();
                        let effect_mask = if n.blending.layer_mask_hides_effects
                            && node.mask.is_some()
                        {
                            let mut mask_node = node.clone();
                            mask_node.opacity = 1.0;
                            mask_node.blend = emulsion_raster::BlendMode::Normal;
                            mask_node.blending = Default::default();
                            mask_node.content = match &node.content {
                                NodeContent::Pixels { raster, placement } => NodeContent::Pixels {
                                    raster: Arc::new(emulsion_raster::Raster::solid(
                                        raster.width(),
                                        raster.height(),
                                        [1.0; 4],
                                    )),
                                    placement: *placement,
                                },
                                _ => NodeContent::Fill([1.0; 4]),
                            };
                            Some(Box::new(mask_node))
                        } else {
                            None
                        };
                        let mut children = Vec::with_capacity(fx.below.len() + fx.above.len() + 1);
                        let effect = |id, rendered: &crate::styles::RenderedEffect| {
                            let mut effect = crate::styles::effect_node(
                                id,
                                rendered.raster.clone(),
                                rendered.rect,
                                n,
                            );
                            effect.opacity = 1.0;
                            effect.blending = Default::default();
                            effect.blend = rendered.blend;
                            effect
                        };
                        for (index, rendered) in fx.below.iter().enumerate() {
                            children.push(effect(
                                n.id.wrapping_mul(32) ^ (1 << 62) ^ index as u64,
                                rendered,
                            ));
                        }
                        node.opacity = 1.0;
                        node.blend = if n.blending.blend_interior_effects_as_group {
                            emulsion_raster::BlendMode::Normal
                        } else {
                            n.blend
                        };
                        node.blending = emulsion_raster::composite::BlendingOptions {
                            fill_opacity: n.blending.fill_opacity,
                            ..Default::default()
                        };
                        if effect_mask.is_some() {
                            node.mask = None;
                        }
                        children.push(node);
                        for (index, rendered) in fx.above.iter().enumerate() {
                            children.push(effect(
                                n.id.wrapping_mul(32) ^ (1 << 63) ^ index as u64,
                                rendered,
                            ));
                        }
                        node = CompositeNode {
                            id: n.id,
                            visible: n.visible,
                            opacity: n.opacity,
                            blend: if !n.blending.blend_interior_effects_as_group
                                || n.blend == emulsion_raster::BlendMode::PassThrough
                            {
                                emulsion_raster::BlendMode::Normal
                            } else {
                                n.blend
                            },
                            blending: emulsion_raster::composite::BlendingOptions {
                                fill_opacity: 1.0,
                                ..n.blending
                            },
                            mask: None,
                            clip_to: None,
                            content: NodeContent::StyledGroup {
                                children,
                                clip_source: Box::new(clip_source),
                                effect_mask,
                            },
                        };
                    }
                    node.clip_to = n.clip_to.and_then(|c| ids.iter().position(|id| *id == c));
                    node
                })
                .collect()
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
                | NodeKind::Text { .. }
                | NodeKind::Smart { .. }
        ) {
            // Mask-only nodes: the mask is already in document space.
            return Document::composite_mask(n).map(|m| (*m).clone());
        }
        let mut solo = Document::new(self.width, self.height);
        solo.global_light = self.global_light;
        solo.blend_space = self.blend_space;
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
        self.buffers_once(&mut std::collections::HashSet::new())
    }

    /// Count a shared plane once across an entire history scan. Duplicating a
    /// large layer must not enumerate its tiles again for every undo snapshot.
    pub(crate) fn buffers_once(
        &self,
        planes: &mut std::collections::HashSet<usize>,
    ) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut raster = |r: &emulsion_raster::Raster| {
            if planes.insert(r as *const _ as usize) {
                out.extend(r.buffer_allocations());
            }
        };
        for n in &self.nodes {
            match &n.kind {
                NodeKind::Raster { raster: r, .. } => raster(r),
                NodeKind::Path { cache, .. } | NodeKind::Text { cache, .. } => raster(cache),
                NodeKind::Smart { source, cache, .. } => {
                    raster(source);
                    raster(cache);
                }
                _ => {}
            }
        }
        for n in &self.nodes {
            if let Some(mask) = &n.mask
                && planes.insert(Arc::as_ptr(mask) as usize)
            {
                out.extend(mask.buffer_allocations());
            }
            for option in &n.style_options {
                if let Some(image) = &option.pattern.image
                    && planes.insert(Arc::as_ptr(image) as usize)
                {
                    out.push((image.pixels.as_ptr() as usize, image.pixels.len()));
                }
            }
        }
        if let Some(selection) = &self.selection
            && planes.insert(Arc::as_ptr(selection) as usize)
        {
            out.extend(selection.buffer_allocations());
        }
        out
    }
}

#[cfg(test)]
mod styled_blending_tests {
    use super::*;
    use crate::styles::LayerStyle;
    use emulsion_raster::composite::{BlendIfChannel, flatten};
    use emulsion_raster::{Placement, Raster};
    fn styled() -> Document {
        let mut doc = Document::new(4, 4);
        let mut node = Node::raster(
            1,
            "styled",
            Arc::new(Raster::from_fn(4, 4, [0; 4], |_, _| {
                [65535, 65535, 65535, 65535]
            })),
            Placement::default(),
        );
        node.styles = vec![LayerStyle::ColorOverlay {
            color: [255, 0, 0],
            opacity: 100.0,
        }];
        doc.nodes.push(node);
        doc
    }
    #[test]
    fn styled_clipping_uses_original_shape_even_at_zero_fill() {
        let mut doc = Document::new(32, 16);
        let mut base = Node::raster(
            1,
            "shape",
            Arc::new(Raster::from_fn(32, 16, [0; 4], |x, y| {
                if (4..12).contains(&x) && (4..12).contains(&y) {
                    [65535; 4]
                } else {
                    [0; 4]
                }
            })),
            Placement::default(),
        );
        base.blending.fill_opacity = 0.0;
        base.styles = vec![LayerStyle::DropShadow {
            color: [0, 0, 0],
            opacity: 100.0,
            angle: 180.0,
            distance: 12.0,
            size: 0.0,
        }];
        doc.nodes.push(base);
        let mut clipped = Node::raster(
            2,
            "clipped red",
            Arc::new(Raster::from_fn(32, 16, [0; 4], |_, _| [65535, 0, 0, 65535])),
            Placement::default(),
        );
        clipped.clip_to = Some(1);
        doc.nodes.push(clipped);
        let flat = flatten(&doc.composite_tree(), 0);
        assert_eq!(
            flat.get(8, 8),
            [65535, 0, 0, 65535],
            "Fill zero must retain base clipping shape"
        );
        assert_eq!(
            flat.get(20, 8),
            [0, 0, 0, 65535],
            "shadow must remain black and not receive clipped red layer"
        );
        assert_eq!(flat.get(28, 8), [0; 4]);
    }

    #[test]
    fn styled_opacity_is_applied_once_to_complete_appearance() {
        let mut doc = styled();
        doc.nodes[0].opacity = 0.5;
        let px = flatten(&doc.composite_tree(), 0).get(1, 1);
        assert!((px[3] as i32 - 32768).abs() <= 1, "{px:?}");
        assert!((px[0] as i32 - 32768).abs() <= 1, "{px:?}");
        assert_eq!(px[1], 0);
        doc.nodes[0].blending.fill_opacity = 0.0;
        assert_eq!(
            flatten(&doc.composite_tree(), 0).get(1, 1),
            px,
            "Fill zero retains the overlay with layer opacity applied once"
        );
    }
    #[test]
    fn styled_blend_if_uses_original_backdrop_for_effects() {
        let mut doc = styled();
        let top = &mut doc.nodes[0];
        top.blending.blend_if.channel = BlendIfChannel::Red;
        top.blending.blend_if.backdrop.white = 0.5;
        top.blending.blend_if.backdrop.white_fade = 0.5;
        doc.nodes.insert(
            0,
            Node::raster(
                2,
                "black",
                Arc::new(Raster::from_fn(4, 4, [0; 4], |_, _| [0, 0, 0, 65535])),
                Placement::default(),
            ),
        );
        assert_eq!(
            flatten(&doc.composite_tree(), 0).get(1, 1),
            [65535, 0, 0, 65535],
            "red overlay uses black backdrop, not its own white content"
        );
        doc.nodes[0] = Node::raster(
            2,
            "white",
            Arc::new(Raster::from_fn(4, 4, [0; 4], |_, _| [65535; 4])),
            Placement::default(),
        );
        assert_eq!(
            flatten(&doc.composite_tree(), 0).get(1, 1),
            [65535; 4],
            "white backdrop hides complete styled layer"
        );
    }
}

#[cfg(test)]
mod retained_buffer_tests {
    use super::*;
    use std::collections::HashSet;
    #[test]
    fn duplicated_layers_and_small_edits_share_backing_tiles() {
        let mut doc = Document::new(512, 256);
        let pixels = Arc::new(emulsion_raster::Raster::from_fn(
            512,
            256,
            [0; 4],
            |_, _| [1, 2, 3, 65535],
        ));
        doc.nodes.push(Node::raster(
            1,
            "Pixels",
            pixels.clone(),
            Default::default(),
        ));
        doc.next_id = 2;
        let initial: HashSet<_> = doc.buffers().into_iter().collect();
        crate::Command::DuplicateNode { id: 1 }
            .apply(&mut doc)
            .unwrap();
        assert_eq!(doc.buffers().into_iter().collect::<HashSet<_>>(), initial);
        let mut planes = HashSet::new();
        assert_eq!(doc.buffers_once(&mut planes).len(), initial.len());
        assert!(
            doc.clone().buffers_once(&mut planes).is_empty(),
            "shared snapshots do not rescan the same image tiles"
        );
        let changed = Arc::new(
            pixels.write_rect(emulsion_raster::IRect::new(0, 0, 1, 1), &[[9, 8, 7, 65535]]),
        );
        crate::Command::ReplacePixels {
            id: 1,
            raster: changed,
            dirty: emulsion_raster::IRect::new(0, 0, 1, 1),
            label: "Pixel".into(),
        }
        .apply(&mut doc)
        .unwrap();
        let after: HashSet<_> = doc.buffers().into_iter().collect();
        assert_eq!(after.len(), 3, "two original tiles plus one edited tile");
        assert_eq!(after.difference(&initial).count(), 1);
        doc.selection = Some(Arc::new(Mask::from_fn(512, 256, 0, |x, _| {
            if x == 0 { 255 } else { 0 }
        })));
        assert!(doc.buffers().len() > after.len());
    }
}
