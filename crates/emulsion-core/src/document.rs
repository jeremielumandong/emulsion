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
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DocumentError {
    #[error("node {0}: {1}")]
    Geometry(NodeId, crate::GeometryError),
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
    #[error("invalid RAW recipe: {0}")]
    BadRaw(&'static str),
    #[error("invalid design settings: {0}")]
    BadDesign(String),
    #[error("invalid diagram: {0}")]
    BadDiagram(String),
    #[error("more than {0} nodes")]
    TooManyNodes(usize),
}

pub const MAX_SIDE: u32 = 30_000;
pub const MAX_PIXELS: u64 = 400_000_000;
pub const MAX_DEPTH: usize = 64;
pub const MAX_NODES: usize = 150_000;

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
    pub diagram: Option<Arc<crate::diagram::Diagram>>,
    pub design: crate::design_metadata::Design,
    pub global_light: crate::style_options::GlobalLight,
    /// Bit depth of the source the document came from (8 or 16), for the
    /// title bar and export defaults. Storage is always 16-bit linear.
    pub source_depth: u8,
    pub blend_space: BlendSpace,
    /// Explicit PSD Background identity. Never inferred from layer appearance.
    pub psd_background: Option<NodeId>,
    /// Bottom to top.
    pub nodes: Vec<Node>,
    pub next_id: NodeId,
    /// Document-space coverage; `None` means no selection (everything).
    pub selection: Option<Arc<emulsion_raster::Mask>>,
    /// Ruler guides, for snapping and alignment. Not rendered into pixels.
    pub guides: Vec<Guide>,
    /// What the camera recorded, when the document came from a photograph.
    pub info: Option<ImageInfo>,
    pub raw: Option<crate::raw::RawDocument>,
    /// Original files remain protected even after baking or painting detaches a recipe.
    pub raw_originals: Vec<std::path::PathBuf>,
    /// Colours painted with in this project, most recent first, for the
    /// draw palette. Not rendered into pixels and not part of history.
    pub colors: Vec<[u8; 3]>,
    /// Drawing Assist guides, the ruler and saved guide sets. Like `colors`,
    /// not rendered and not part of history.
    pub drawing_guides: crate::drawing_guides::DrawingGuides,
}

/// How many painted colours a project remembers.
pub const MAX_PROJECT_COLORS: usize = 32;

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
            && self.design == o.design
            && self.diagram == o.diagram
            && self.global_light == o.global_light
            && self.blend_space == o.blend_space
            && self.psd_background == o.psd_background
            && self.nodes == o.nodes
            && self.guides == o.guides
            && self.raw == o.raw
            && self.raw_originals == o.raw_originals
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
            diagram: None,
            design: Default::default(),
            source_depth: 8,
            blend_space: BlendSpace::Linear,
            psd_background: None,
            nodes: Vec::new(),
            next_id: 1,
            selection: None,
            guides: Vec::new(),
            info: None,
            raw: None,
            raw_originals: Vec::new(),
            colors: Vec::new(),
            drawing_guides: Default::default(),
        }
    }

    /// Remember a colour painted with: moves it to the front, keeping at
    /// most [`MAX_PROJECT_COLORS`]. Returns whether the palette changed.
    pub fn note_color(&mut self, rgb: [u8; 3]) -> bool {
        if self.colors.first() == Some(&rgb) {
            return false;
        }
        self.colors.retain(|c| *c != rgb);
        self.colors.insert(0, rgb);
        self.colors.truncate(MAX_PROJECT_COLORS);
        true
    }

    pub fn alloc_id(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Protection is monotonic across history navigation, even when a recipe
    /// or relink operation is undone.
    pub(crate) fn retain_raw_originals(&mut self, previous: &Document) {
        for path in previous
            .raw_originals
            .iter()
            .chain(previous.raw.iter().map(|raw| &raw.source))
        {
            if !self.raw_originals.contains(path) {
                self.raw_originals.push(path.clone());
            }
        }
    }

    /// Preview replay/cancel may restore a deliberately unnormalized valid
    /// recipe. Do not add a redundant protected-path entry for an unchanged RAW
    /// recipe/source merely because geometry was previewed. Actual source or
    /// recipe changes still take the monotonic retention path, and independently
    /// introduced protected paths are always retained.
    pub(crate) fn retain_preview_raw_originals(&mut self, previous: &Document) {
        fn source(doc: &Document, id: NodeId) -> Option<&Arc<emulsion_raster::Raster>> {
            match &doc.node(id)?.kind {
                NodeKind::Raster { raster, .. } => Some(raster),
                NodeKind::Smart {
                    source,
                    editable: None,
                    ..
                } => Some(source),
                _ => None,
            }
        }
        let unchanged = match (&self.raw, &previous.raw) {
            (Some(a), Some(b)) if a == b => {
                match (source(self, a.node_id), source(previous, b.node_id)) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    _ => false,
                }
            }
            _ => false,
        };
        if !unchanged {
            self.retain_raw_originals(previous);
            return;
        }
        let raw_path = &self.raw.as_ref().expect("unchanged RAW recipe").source;
        for path in &previous.raw_originals {
            if path != raw_path && !self.raw_originals.contains(path) {
                self.raw_originals.push(path.clone());
            }
        }
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

    /// Effective coverage for thumbnails/inspection, even when disabled.
    /// Smart results use the expanded filter-cache grid and cache placement.
    pub fn mask_for_inspection(
        &self,
        node: &Node,
    ) -> Result<Option<Arc<Mask>>, crate::GeometryError> {
        self.raster_mask_for_inspection(node)
    }

    /// Raster component only, ignoring its enabled state.
    pub fn raster_mask_for_inspection(
        &self,
        node: &Node,
    ) -> Result<Option<Arc<Mask>>, crate::GeometryError> {
        crate::composite_mask_cache::mask_for_inspection(node, (self.width, self.height))
    }
    /// Vector component only, ignoring its enabled state.
    pub fn vector_mask_for_inspection(
        &self,
        node: &Node,
    ) -> Result<Option<Arc<Mask>>, crate::GeometryError> {
        crate::composite_mask_cache::vector_mask_for_inspection(node, (self.width, self.height))
    }

    /// Enabled effective coverage: intrinsic feather/density, then affine.
    /// Smart masks are shifted from source space into their filter-cache grid.
    pub fn composite_mask(&self, node: &Node) -> Result<Option<Arc<Mask>>, crate::GeometryError> {
        crate::composite_mask_cache::composite_mask(node, (self.width, self.height))
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
        let mut kids: HashMap<Option<NodeId>, Vec<NodeId>> = HashMap::new();
        let mut unique = std::collections::HashSet::with_capacity(self.nodes.len());
        for n in &self.nodes {
            // Leave malformed input intact for validation instead of panicking
            // while transferring nodes out of the lookup table.
            if !unique.insert(n.id) {
                return;
            }
            kids.entry(n.parent).or_default().push(n.id);
        }
        let mut out = Vec::with_capacity(self.nodes.len());
        // Reordering transfers ownership; cloning every vector/text node twice made
        // an otherwise local diagram drag scale with all artwork in the page.
        let mut by_id: HashMap<NodeId, Node> = std::mem::take(&mut self.nodes)
            .into_iter()
            .map(|n| (n.id, n))
            .collect();
        fn emit(
            id: NodeId,
            kids: &HashMap<Option<NodeId>, Vec<NodeId>>,
            by_id: &mut HashMap<NodeId, Node>,
            out: &mut Vec<Node>,
        ) {
            if let Some(ch) = kids.get(&Some(id)) {
                for c in ch {
                    emit(*c, kids, by_id, out);
                }
            }
            out.push(by_id.remove(&id).expect("normalized node exists"));
        }
        if let Some(roots) = kids.get(&None) {
            for r in roots {
                emit(*r, &kids, &mut by_id, &mut out);
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

    /// Only a bottom root raster without a clip link can retain this role.
    /// Visibility, names, locks and painted pixels do not determine identity.
    pub fn valid_psd_background(&self, id: NodeId) -> bool {
        self.nodes
            .iter()
            .find(|node| node.parent.is_none())
            .is_some_and(|node| {
                node.id == id
                    && node.clip_to.is_none()
                    && matches!(node.kind, NodeKind::Raster { .. })
            })
    }

    /// Editing may demote a Background; loading must validate without pruning.
    pub fn prune_psd_background(&mut self) {
        if self
            .psd_background
            .is_some_and(|id| !self.valid_psd_background(id))
        {
            self.psd_background = None;
        }
    }

    pub fn validate(&self) -> Result<(), DocumentError> {
        if let Some(id) = self.psd_background
            && !self.valid_psd_background(id)
        {
            return Err(DocumentError::BadValue(id, "PSD Background target"));
        }
        if self.raw_originals.len() > 1024
            || self
                .raw_originals
                .iter()
                .any(|path| path.as_os_str().is_empty())
        {
            return Err(DocumentError::BadRaw(
                "invalid original-file protection list",
            ));
        }
        if let Some(raw) = &self.raw {
            raw.validate().map_err(DocumentError::BadRaw)?;
            if !self.node(raw.node_id).is_some_and(|node| {
                matches!(
                    node.kind,
                    NodeKind::Raster { .. } | NodeKind::Smart { editable: None, .. }
                )
            }) {
                return Err(DocumentError::BadRaw(
                    "RAW source node is missing or incompatible",
                ));
            }
        }
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
        let mut indices = HashMap::with_capacity(self.nodes.len());
        for (i, n) in self.nodes.iter().enumerate() {
            if let NodeKind::Strokes { strokes, .. } = &n.kind
                && strokes.validate().is_err()
            {
                return Err(DocumentError::BadValue(n.id, "vector strokes"));
            }
            if indices.insert(n.id, i).is_some() {
                return Err(DocumentError::DuplicateId(n.id));
            }
        }
        for n in &self.nodes {
            crate::smart_support::validate_node(n).map_err(|e| DocumentError::Geometry(n.id, e))?;
            if matches!(
                &n.kind,
                NodeKind::Smart {
                    editable: Some(_),
                    original_image: Some(_),
                    ..
                }
            ) {
                return Err(DocumentError::BadValue(
                    n.id,
                    "original PNG requires a raster-backed Smart source",
                ));
            }
            if let NodeKind::Smart {
                editable: Some(crate::node::SmartEditable::Document { archive, external }),
                ..
            } = &n.kind
            {
                if archive.is_empty() || archive.len() > crate::smart_source::MAX_SOURCE_BYTES {
                    return Err(DocumentError::BadValue(n.id, "Smart source archive size"));
                }
                if external.as_ref().is_some_and(|l| l.validate().is_err()) {
                    return Err(DocumentError::BadValue(n.id, "Smart source link"));
                }
            }

            if let Some(p) = n.parent {
                match indices.get(&p).map(|i| &self.nodes[*i]) {
                    None => return Err(DocumentError::MissingParent(n.id, p)),
                    Some(pn) if !pn.is_group() => {
                        return Err(DocumentError::ParentNotGroup(n.id, p));
                    }
                    _ => {}
                }
            }
            if let NodeKind::Smart {
                filters,
                filter_styles,
                ..
            } = &n.kind
                && filter_styles.len() > filters.len()
            {
                return Err(DocumentError::BadValue(n.id, "orphan filter styles"));
            }
            if n.style_options.len() > n.styles.len() || n.style_options.iter().any(|o| !o.valid())
            {
                return Err(DocumentError::BadValue(n.id, "style options"));
            }
            if let NodeKind::Smart {
                filter_mask: Some(mask),
                cache,
                ..
            } = &n.kind
                && (!mask.valid()
                    || cache.width() == 0
                    || cache.height() == 0
                    || cache.width() > MAX_SIDE
                    || cache.height() > MAX_SIDE
                    || u64::from(cache.width()) * u64::from(cache.height()) > MAX_PIXELS)
            {
                return Err(DocumentError::BadValue(n.id, "Smart Filter mask"));
            }
            if let Some(mask) = &n.vector_mask {
                if !mask.valid() {
                    return Err(DocumentError::BadValue(n.id, "vector mask"));
                }
                let (width, height, offset) =
                    crate::composite_mask_cache::output_grid(n, (self.width, self.height))
                        .map_err(|e| DocumentError::Geometry(n.id, e))?;
                crate::vector_mask::validate_render(mask, (width, height), offset)
                    .map_err(|message| DocumentError::BadValue(n.id, message))?;
            }
            if !n.mask_properties.valid() {
                return Err(DocumentError::BadValue(n.id, "mask properties"));
            }
            if let crate::Mapping2::Affine(mask_affine) = n.mask_transform
                && (!mask_affine.is_finite() || mask_affine.matrix2.determinant().abs() < 1e-12)
            {
                return Err(DocumentError::BadValue(n.id, "mask transform"));
            }
            if !n.blending.valid() {
                return Err(DocumentError::BadValue(n.id, "blending options"));
            }
            if !(0.0..=1.0).contains(&n.opacity) || !n.opacity.is_finite() {
                return Err(DocumentError::BadValue(n.id, "opacity"));
            }
            if let Some(placement) = match &n.kind {
                NodeKind::Raster { placement, .. } => Some(placement),
                NodeKind::Smart {
                    placement: crate::SmartPlacement::Legacy(placement),
                    ..
                } => Some(placement),
                _ => None,
            } {
                let p = placement;
                let finite = [p.x, p.y, p.scale_x, p.scale_y, p.rotation]
                    .iter()
                    .all(|v| v.is_finite());
                if !finite || p.scale_x.abs() < 1e-6 || p.scale_y.abs() < 1e-6 {
                    return Err(DocumentError::BadValue(n.id, "placement"));
                }
            }
            if let Some(mask) = &n.mask
                && (mask.width() == 0
                    || mask.height() == 0
                    || mask.width() > MAX_SIDE
                    || mask.height() > MAX_SIDE
                    || u64::from(mask.width()) * u64::from(mask.height()) > MAX_PIXELS)
            {
                return Err(DocumentError::BadValue(n.id, "mask size"));
            }
            if let NodeKind::Adjust(a) = &n.kind
                && a.params().iter().any(|s| !s.value.is_finite())
            {
                return Err(DocumentError::BadValue(n.id, "adjustment"));
            }
        }
        // Accumulate ancestor intervals in O(nodes × bounded depth). Avoid
        // repeatedly scanning entire subtrees when validating large diagrams.
        let mut descendants = vec![(0usize, usize::MAX, 0usize); self.nodes.len()];
        for (i, n) in self.nodes.iter().enumerate() {
            let mut parent = n.parent;
            let mut depth = 0;
            while let Some(id) = parent {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(DocumentError::TooDeep(MAX_DEPTH));
                }
                let index = indices[&id];
                let count = &mut descendants[index];
                count.0 += 1;
                count.1 = count.1.min(i);
                count.2 = count.2.max(i);
                parent = self.nodes[index].parent;
            }
        }
        for (i, n) in self.nodes.iter().enumerate() {
            let (count, first, last) = descendants[i];
            if n.is_group() && count > 0 && (count > i || first != i - count || last != i - 1) {
                return Err(DocumentError::NotContiguous(n.id));
            }
            if let Some(c) = n.clip_to {
                match indices.get(&c).copied() {
                    Some(base) if base < i && self.nodes[base].parent == n.parent => {}
                    _ => return Err(DocumentError::BadClip(n.id, c)),
                }
            }
        }
        self.design
            .validate(self)
            .map_err(DocumentError::BadDesign)?;
        if let Some(diagram) = &self.diagram {
            diagram.validate(self).map_err(DocumentError::BadDiagram)?;
        }
        crate::styles::validate_projective_effect_resources(self)?;
        Ok(())
    }

    /// Node panel rows, top to bottom, skipping children of collapsed groups.
    pub fn panel_rows(&self) -> Vec<PanelRow> {
        self.panel_rows_with(false, |_| true)
    }

    /// Matching panel rows, including descendants of collapsed groups, without
    /// changing expansion state. Ancestors need not match for a child to match.
    pub fn filter_panel_rows(&self, matches: impl Fn(&Node) -> bool) -> Vec<PanelRow> {
        self.panel_rows_with(true, matches)
    }

    fn panel_rows_with(
        &self,
        include_collapsed: bool,
        matches: impl Fn(&Node) -> bool,
    ) -> Vec<PanelRow> {
        let mut out = Vec::new();
        let mut children: HashMap<Option<NodeId>, Vec<&Node>> = HashMap::new();
        for node in &self.nodes {
            children.entry(node.parent).or_default().push(node);
        }
        fn walk(
            children: &HashMap<Option<NodeId>, Vec<&Node>>,
            parent: Option<NodeId>,
            depth: usize,
            out: &mut Vec<PanelRow>,
            include_collapsed: bool,
            matches: &impl Fn(&Node) -> bool,
        ) {
            for node in children.get(&parent).into_iter().flatten().rev() {
                if matches(node) {
                    out.push(PanelRow { id: node.id, depth });
                }
                if let NodeKind::Group { collapsed } = node.kind
                    && (include_collapsed || !collapsed)
                {
                    walk(
                        children,
                        Some(node.id),
                        depth + 1,
                        out,
                        include_collapsed,
                        matches,
                    );
                }
            }
        }
        walk(&children, None, 0, &mut out, include_collapsed, &matches);
        out
    }

    /// Compatibility wrapper for callers that explicitly require legacy state.
    /// Projective-capable consumers must use try_composite_tree and surface errors.
    pub fn composite_tree(&self) -> CompositeTree {
        assert!(
            !self.nodes.iter().any(Node::has_projective_metadata),
            "projective documents require try_composite_tree"
        );
        self.try_composite_tree()
            .expect("validated legacy render document")
    }

    /// Prepare and validate a complete render tree before any publication.
    pub fn try_composite_tree(&self) -> Result<CompositeTree, DocumentError> {
        if self.nodes.iter().any(Node::has_projective_metadata) {
            self.validate()?;
        }
        let mut children: HashMap<Option<NodeId>, Vec<&Node>> = HashMap::new();
        for node in &self.nodes {
            children.entry(node.parent).or_default().push(node);
        }
        fn build(
            doc: &Document,
            children: &HashMap<Option<NodeId>, Vec<&Node>>,
            parent: Option<NodeId>,
        ) -> Result<Vec<CompositeNode>, DocumentError> {
            let siblings = children.get(&parent).map(Vec::as_slice).unwrap_or(&[]);
            let positions: HashMap<_, _> = siblings
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id, i))
                .collect();
            siblings
                .iter()
                .copied()
                .map(|n| {
                    let content = match &n.kind {
                        NodeKind::Raster { raster, placement } => NodeContent::Pixels {
                            raster: raster.clone().into(),
                            placement: *placement,
                        },
                        NodeKind::Group { .. } => {
                            NodeContent::Group(crate::design_clipping::composite_children(
                                doc,
                                n.id,
                                build(doc, children, Some(n.id))?,
                            ))
                        }
                        NodeKind::Adjust(a) => NodeContent::Adjust(Arc::new(a.prepare())),
                        NodeKind::Fill { rgba } => {
                            NodeContent::Fill(color::srgba8_to_premul(*rgba))
                        }
                        NodeKind::Path { cache, .. }
                        | NodeKind::Text { cache, .. }
                        | NodeKind::Strokes { cache, .. } => {
                            // Defer: a renderer that draws the vector itself
                            // never needs these, and rendering them costs
                            // tens of milliseconds on a large document.
                            let cache = cache.clone();
                            NodeContent::Pixels {
                                raster: emulsion_raster::composite::LazyRaster::deferred_with_id(
                                    cache.size(),
                                    cache.id(),
                                    Arc::new(move || cache.pixels().clone()),
                                ),
                                placement: emulsion_raster::Placement::default(),
                            }
                        }
                        NodeKind::Smart {
                            source, placement, ..
                        } => {
                            let raster = crate::smart_filter_mask::effective_pixels_with_space(
                                n,
                                doc.blend_space,
                            )
                            .map_err(|e| DocumentError::Geometry(n.id, e))?
                            .expect("Smart node");
                            let grid = crate::smart_support::output_grid(n)
                                .map_err(|e| DocumentError::Geometry(n.id, e))?;
                            match *placement {
                                crate::SmartPlacement::Legacy(p) => NodeContent::Pixels {
                                    raster: raster.into(),
                                    placement: crate::smart::cache_placement(
                                        &p,
                                        (source.width(), source.height()),
                                        grid.size,
                                        grid.offset,
                                    ),
                                },
                                crate::SmartPlacement::Projective(h) => {
                                    NodeContent::projective_pixels(
                                        raster.into(),
                                        crate::Mapping2::Projective(h)
                                            .with_source_offset(grid.offset)
                                            .map_err(|e| DocumentError::Geometry(n.id, e.into()))?
                                            .to_projective()
                                            .map_err(|e| DocumentError::Geometry(n.id, e.into()))?,
                                    )
                                    .map_err(|e| DocumentError::Geometry(n.id, e.into()))?
                                }
                            }
                        }
                    };
                    Ok((
                        n,
                        CompositeNode {
                            id: n.id,
                            visible: n.visible,
                            opacity: n.opacity,
                            blend: n.blend,
                            blending: n.blending,
                            mask: doc
                                .composite_mask(n)
                                .map_err(|e| DocumentError::Geometry(n.id, e))?,
                            clip_to: None,
                            clip_rect: None,
                            content,
                        },
                    ))
                })
                .collect::<Result<Vec<_>, DocumentError>>()?
                .into_iter()
                .map(|(n, mut node)| {
                    // Composite the complete styled appearance once against the real
                    // backdrop. Fill affects content only; layer opacity, channels,
                    // blend mode and tonal gates affect content and effects together.
                    if let Some(fx) = if n.visible {
                        crate::styles::try_render(doc, n)?
                    } else {
                        None
                    } {
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
                                NodeContent::Pixels { raster, placement } => {
                                    let (raster, placement) = (raster.clone(), *placement);
                                    NodeContent::Pixels {
                                        raster: emulsion_raster::composite::LazyRaster::deferred(
                                            raster.size(),
                                            Arc::new(move || {
                                                let r = raster.get();
                                                Arc::new(emulsion_raster::Raster::solid(
                                                    r.width(),
                                                    r.height(),
                                                    [1.0; 4],
                                                ))
                                            }),
                                        ),
                                        placement,
                                    }
                                }
                                NodeContent::ProjectivePixels(pixels) => {
                                    let (w, h) = pixels.raster().size();
                                    NodeContent::projective_pixels(
                                        Arc::new(emulsion_raster::Raster::solid(w, h, [1.0; 4]))
                                            .into(),
                                        pixels.mapping().forward(),
                                    )
                                    .map_err(|e| DocumentError::Geometry(n.id, e.into()))?
                                }
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
                            clip_rect: None,
                            content: NodeContent::StyledGroup {
                                children,
                                clip_source: Box::new(clip_source),
                                effect_mask,
                            },
                        };
                    }
                    crate::design_background::composite_boundary(doc, n, &mut node);
                    node.clip_to = n.clip_to.and_then(|c| positions.get(&c).copied());
                    Ok(node)
                })
                .collect()
        }
        let tree = CompositeTree {
            width: self.width,
            height: self.height,
            space: self.blend_space,
            knockout_background: self.psd_background,
            nodes: build(self, &children, None)?,
        };
        tree.validate_projective_resources()
            .map_err(|e| DocumentError::Geometry(0, e.into()))?;
        Ok(tree)
    }

    /// Distinct pixel buffers referenced by this document, for memory
    /// accounting.
    /// A node's visible coverage as a document-space selection: its pixels'
    /// alpha (through its mask) for pixel and fill nodes, or its mask for
    /// adjustments and groups. None when the node covers nothing.
    pub fn node_coverage(
        &self,
        id: NodeId,
    ) -> Result<Option<emulsion_raster::Mask>, DocumentError> {
        let Some(n) = self.node(id) else {
            return Ok(None);
        };
        if !matches!(
            n.kind,
            NodeKind::Raster { .. }
                | NodeKind::Fill { .. }
                | NodeKind::Path { .. }
                | NodeKind::Strokes { .. }
                | NodeKind::Text { .. }
                | NodeKind::Smart { .. }
        ) {
            // Mask-only nodes: the mask is already in document space.
            return Ok(self
                .composite_mask(n)
                .map_err(|e| DocumentError::Geometry(id, e))?
                .map(|m| (*m).clone()));
        }
        let solo = self.solo(id).expect("existing node");
        let full = emulsion_raster::composite::flatten(&solo.try_composite_tree()?, 0);
        let region = full.tile_bounds();
        let px = full.read_rect(region);
        let alpha: Vec<u8> = px
            .iter()
            .map(|p| (color::u16_to_f(p[3]) * 255.0).round() as u8)
            .collect();
        let m = emulsion_raster::Mask::empty(self.width, self.height, 0).write_rect(region, &alpha);
        Ok((!emulsion_raster::select::bounds(&m).is_empty()).then_some(m))
    }

    /// A document holding only node `id`, shown at full opacity in Normal
    /// blend at the top level: how that layer looks on its own.
    pub fn solo(&self, id: NodeId) -> Option<Document> {
        let mut node = self.node(id)?.clone();
        let mut solo = Document::new(self.width, self.height);
        solo.global_light = self.global_light;
        solo.blend_space = self.blend_space;
        node.parent = None;
        node.clip_to = None;
        node.visible = true;
        node.opacity = 1.0;
        node.blend = emulsion_raster::BlendMode::Normal;
        solo.nodes.push(node);
        Some(solo)
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
        for font in self.design.fonts.values() {
            let allocation = font.allocation();
            if planes.insert(allocation.0) {
                out.push(allocation);
            }
        }
        let mut raster = |r: &emulsion_raster::Raster| {
            if planes.insert(r as *const _ as usize) {
                out.extend(r.buffer_allocations());
            }
        };
        for n in &self.nodes {
            match &n.kind {
                NodeKind::Raster { raster: r, .. } => raster(r),
                NodeKind::Path { cache, .. }
                | NodeKind::Text { cache, .. }
                | NodeKind::Strokes { cache, .. } => {
                    if let Some(pixels) = cache.rendered_pixels() {
                        raster(pixels);
                    }
                }
                NodeKind::Smart { source, cache, .. } => {
                    raster(source);
                    raster(cache);
                }
                _ => {}
            }
        }
        for n in &self.nodes {
            if let NodeKind::Smart {
                original_image: Some(original),
                ..
            } = &n.kind
            {
                let bytes = original.bytes();
                let allocation = (Arc::as_ptr(bytes) as usize, bytes.capacity());
                if planes.insert(allocation.0) {
                    out.push(allocation);
                }
            }
            if let NodeKind::Smart {
                editable: Some(crate::node::SmartEditable::Svg { xml }),
                ..
            } = &n.kind
            {
                let allocation = (xml.as_ptr() as usize, xml.len());
                if planes.insert(allocation.0) {
                    out.push(allocation);
                }
            }
            if let NodeKind::Smart {
                editable: Some(crate::node::SmartEditable::Document { archive, .. }),
                ..
            } = &n.kind
            {
                let allocation = (Arc::as_ptr(archive) as usize, archive.capacity());
                if planes.insert(allocation.0) {
                    out.push(allocation);
                }
            }
            if let Some(mask) = &n.mask
                && planes.insert(Arc::as_ptr(mask) as usize)
            {
                out.extend(mask.buffer_allocations());
            }
            if let Some(mask) = crate::smart_filter_mask::descriptor(n)
                && planes.insert(Arc::as_ptr(&mask.pixels) as usize)
            {
                out.extend(mask.pixels.buffer_allocations());
            }
            // Count authoritative vector-mask geometry without changing the
            // existing lazy-content contract: ordinary Path nodes contribute
            // pixel buffers only after their cache has actually rendered.
            if let Some(mask) = &n.vector_mask
                && planes.insert(Arc::as_ptr(&mask.path) as usize)
            {
                let path = &mask.path;
                out.push((
                    Arc::as_ptr(path) as usize,
                    std::mem::size_of::<emulsion_raster::vector::Path>(),
                ));
                out.push((
                    path.subpaths.as_ptr() as usize,
                    path.subpaths.capacity()
                        * std::mem::size_of::<emulsion_raster::vector::SubPath>(),
                ));
                for subpath in &path.subpaths {
                    if subpath.anchors.capacity() > 0 {
                        out.push((
                            subpath.anchors.as_ptr() as usize,
                            subpath.anchors.capacity()
                                * std::mem::size_of::<emulsion_raster::vector::Anchor>(),
                        ));
                    }
                }
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
        for media in self.design.local_media.values() {
            if planes.insert(Arc::as_ptr(&media.bytes) as usize) {
                out.push((media.bytes.as_ptr() as usize, media.bytes.len()));
            }
        }
        out
    }
}

#[cfg(test)]
mod panel_search_tests {
    use super::*;

    #[test]
    fn filtering_preserves_order_depth_and_collapsed_state() {
        let mut doc = Document::new(32, 32);
        let mut inner = Node::group(2, "Matching inner");
        inner.parent = Some(3);
        inner.kind = NodeKind::Group { collapsed: true };
        let mut child = Node::group(1, "Matching child");
        child.parent = Some(2);
        let mut outer = Node::group(3, "Other outer");
        outer.kind = NodeKind::Group { collapsed: true };
        doc.nodes = vec![child, inner, outer, Node::group(4, "Matching root")];
        let before = doc.clone();
        assert_eq!(
            doc.panel_rows(),
            vec![PanelRow { id: 4, depth: 0 }, PanelRow { id: 3, depth: 0 }]
        );
        assert_eq!(
            doc.filter_panel_rows(|node| node.name.contains("Matching")),
            vec![
                PanelRow { id: 4, depth: 0 },
                PanelRow { id: 2, depth: 1 },
                PanelRow { id: 1, depth: 2 },
            ]
        );
        assert!(doc.filter_panel_rows(|_| false).is_empty());
        assert_eq!(doc, before);
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

#[cfg(test)]
mod project_color_tests {
    use super::*;

    #[test]
    fn painted_colors_are_most_recent_first_without_duplicates() {
        let mut doc = Document::new(4, 4);
        assert!(doc.note_color([1, 2, 3]));
        assert!(doc.note_color([4, 5, 6]));
        assert!(
            !doc.note_color([4, 5, 6]),
            "repainting the newest is a no-op"
        );
        assert!(doc.note_color([1, 2, 3]));
        assert_eq!(doc.colors, vec![[1, 2, 3], [4, 5, 6]]);
        for i in 0..100u8 {
            doc.note_color([i, i, 0]);
        }
        assert_eq!(doc.colors.len(), MAX_PROJECT_COLORS);
        assert_eq!(doc.colors[0], [99, 99, 0]);
    }
}

#[cfg(test)]
mod hierarchy_validation_tests {
    use super::*;
    #[test]
    fn cyclic_or_interleaved_groups_are_rejected_without_unbounded_walks() {
        let mut doc = Document::new(20, 20);
        let mut a = Node::new(1, "A", NodeKind::Group { collapsed: false });
        a.parent = Some(2);
        let mut b = Node::new(2, "B", NodeKind::Group { collapsed: false });
        b.parent = Some(1);
        doc.nodes = vec![a, b];
        assert_eq!(doc.validate(), Err(DocumentError::TooDeep(MAX_DEPTH)));
        let mut child = Node::new(3, "Child", NodeKind::Fill { rgba: [255; 4] });
        child.parent = Some(1);
        doc.nodes = vec![
            child,
            Node::new(2, "Sibling", NodeKind::Fill { rgba: [255; 4] }),
            Node::new(1, "Parent", NodeKind::Group { collapsed: false }),
        ];
        assert_eq!(doc.validate(), Err(DocumentError::NotContiguous(1)));
        doc.nodes.swap(0, 1);
        doc.validate().unwrap();
        doc.nodes[1].clip_to = Some(2);
        assert_eq!(doc.validate(), Err(DocumentError::BadClip(3, 2)));
    }
}

#[cfg(test)]
mod normalization_regression {
    #[test]
    fn duplicate_ids_survive_normalization_for_validation() {
        let mut doc = crate::diagram_library::TEMPLATES[0].build().unwrap();
        doc.nodes.push(doc.nodes[0].clone());
        let count = doc.nodes.len();
        doc.normalize();
        assert_eq!(doc.nodes.len(), count);
        assert!(doc.validate().is_err());
    }
}
