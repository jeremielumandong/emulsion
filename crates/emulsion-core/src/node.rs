//! Nodes: every operation in a document is a node with named parameters.

use emulsion_raster::{Adjustment, BlendMode, Mask, Placement, Raster};
use std::sync::Arc;

pub type NodeId = u64;

#[derive(Clone, Debug)]
pub enum NodeKind {
    /// Pixels, placed non-destructively.
    Raster { raster: Arc<Raster>, placement: Placement },
    /// A container. Its descendants sit directly below it in the stack.
    Group { collapsed: bool },
    /// An adjustment applied to everything below it in its parent.
    Adjust(Adjustment),
    /// A solid colour, straight sRGB 8-bit plus alpha.
    Fill { rgba: [u8; 4] },
}

impl NodeKind {
    pub fn is_group(&self) -> bool {
        matches!(self, NodeKind::Group { .. })
    }

    /// Short tag for the node panel.
    pub fn tag(&self) -> &'static str {
        match self {
            NodeKind::Raster { .. } => "px",
            NodeKind::Group { .. } => "grp",
            NodeKind::Adjust(_) => "adj",
            NodeKind::Fill { .. } => "fill",
        }
    }
}

impl PartialEq for NodeKind {
    /// Pixel data compares by identity: two nodes are equal only if they
    /// share the same buffer. This keeps equality checks O(1).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (NodeKind::Raster { raster: a, placement: pa }, NodeKind::Raster { raster: b, placement: pb }) => {
                Arc::ptr_eq(a, b) && pa == pb
            }
            (NodeKind::Group { collapsed: a }, NodeKind::Group { collapsed: b }) => a == b,
            (NodeKind::Adjust(a), NodeKind::Adjust(b)) => a == b,
            (NodeKind::Fill { rgba: a }, NodeKind::Fill { rgba: b }) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub parent: Option<NodeId>,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    /// Clip to the content of a sibling below.
    pub clip_to: Option<NodeId>,
    /// Coverage mask. For raster nodes it lives in the node's pixel space and
    /// moves with it; otherwise it is in document space.
    pub mask: Option<Arc<Mask>>,
    pub mask_enabled: bool,
    pub kind: NodeKind,
}

impl PartialEq for Node {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
            && self.name == o.name
            && self.parent == o.parent
            && self.visible == o.visible
            && self.locked == o.locked
            && self.opacity == o.opacity
            && self.blend == o.blend
            && self.clip_to == o.clip_to
            && match (&self.mask, &o.mask) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
            && self.mask_enabled == o.mask_enabled
            && self.kind == o.kind
    }
}

impl Node {
    pub fn new(id: NodeId, name: impl Into<String>, kind: NodeKind) -> Self {
        let blend = if kind.is_group() { BlendMode::PassThrough } else { BlendMode::Normal };
        Self {
            id,
            name: name.into(),
            parent: None,
            visible: true,
            locked: false,
            opacity: 1.0,
            blend,
            clip_to: None,
            mask: None,
            mask_enabled: true,
            kind,
        }
    }

    pub fn raster(id: NodeId, name: impl Into<String>, raster: Arc<Raster>, placement: Placement) -> Self {
        Self::new(id, name, NodeKind::Raster { raster, placement })
    }

    pub fn group(id: NodeId, name: impl Into<String>) -> Self {
        Self::new(id, name, NodeKind::Group { collapsed: false })
    }

    pub fn adjust(id: NodeId, adj: Adjustment) -> Self {
        let name = adj.label().to_string();
        Self::new(id, name, NodeKind::Adjust(adj))
    }

    pub fn is_group(&self) -> bool {
        self.kind.is_group()
    }

    /// Named parameters shown in the node panel.
    pub fn params(&self) -> Vec<emulsion_raster::adjust::ParamSpec> {
        match &self.kind {
            NodeKind::Adjust(a) => a.params(),
            _ => Vec::new(),
        }
    }
}
