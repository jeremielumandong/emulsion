//! Nodes: every operation in a document is a node with named parameters.

use emulsion_raster::vector::{Path, PathStyle};
use emulsion_raster::{Adjustment, BlendMode, Mask, Placement, Raster};
use std::sync::Arc;

pub type NodeId = u64;

/// Independently protected parts of a layer. Group locks are inherited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LayerLocks {
    pub transparency: bool,
    pub pixels: bool,
    pub position: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerColor {
    #[default]
    None,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Violet,
    Gray,
}
impl LayerColor {
    pub const ALL: [Self; 8] = [
        Self::None,
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Violet,
        Self::Gray,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Violet => "Violet",
            Self::Gray => "Gray",
        }
    }
}

/// Original editable content retained by a Smart Object.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SmartEditable {
    Text { spec: Arc<crate::text::TextSpec> },
    Path { path: Arc<Path>, style: PathStyle },
}

#[derive(Clone, Debug)]
pub enum NodeKind {
    /// Pixels, placed non-destructively.
    Raster {
        raster: Arc<Raster>,
        placement: Placement,
    },
    /// A container. Its descendants sit directly below it in the stack.
    Group { collapsed: bool },
    /// An adjustment applied to everything below it in its parent.
    Adjust(Adjustment),
    /// A solid colour, straight sRGB 8-bit plus alpha.
    Fill { rgba: [u8; 4] },
    /// A vector path in document space, rasterized into `cache` whenever it
    /// or its style changes.
    Path {
        path: Arc<Path>,
        style: PathStyle,
        cache: Arc<Raster>,
    },
    /// A text layer, shaped and rasterized into `cache` (see `text`).
    Text {
        spec: Arc<crate::text::TextSpec>,
        cache: Arc<Raster>,
    },
    /// Source pixels with an editable filter stack, rendered into `cache`,
    /// whose top-left sits at `offset` in source pixels (see `smart`).
    Smart {
        editable: Option<SmartEditable>,
        source: Arc<Raster>,
        filters: Vec<emulsion_filters::Filter>,
        filter_styles: Vec<emulsion_filters::FilterStyle>,
        placement: Placement,
        cache: Arc<Raster>,
        offset: (i32, i32),
    },
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
            NodeKind::Path { .. } => "path",
            NodeKind::Text { .. } => "text",
            NodeKind::Smart { .. } => "smart",
        }
    }
}

impl PartialEq for NodeKind {
    /// Pixel data compares by identity: two nodes are equal only if they
    /// share the same buffer. This keeps equality checks O(1).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                NodeKind::Raster {
                    raster: a,
                    placement: pa,
                },
                NodeKind::Raster {
                    raster: b,
                    placement: pb,
                },
            ) => Arc::ptr_eq(a, b) && pa == pb,
            (NodeKind::Group { collapsed: a }, NodeKind::Group { collapsed: b }) => a == b,
            (NodeKind::Adjust(a), NodeKind::Adjust(b)) => a == b,
            (NodeKind::Fill { rgba: a }, NodeKind::Fill { rgba: b }) => a == b,
            (
                NodeKind::Path {
                    path: a, style: sa, ..
                },
                NodeKind::Path {
                    path: b, style: sb, ..
                },
            ) => sa == sb && (Arc::ptr_eq(a, b) || a == b),
            (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) => {
                Arc::ptr_eq(a, b) || a == b
            }
            (
                NodeKind::Smart {
                    source: a,
                    editable: ea,
                    filters: fa,
                    filter_styles: sa,
                    placement: pa,
                    ..
                },
                NodeKind::Smart {
                    source: b,
                    editable: eb,
                    filters: fb,
                    filter_styles: sb,
                    placement: pb,
                    ..
                },
            ) => Arc::ptr_eq(a, b) && ea == eb && fa == fb && sa == sb && pa == pb,
            _ => false,
        }
    }
}

pub fn default_mask_linked() -> bool {
    true
}
pub fn default_mask_transform() -> [f64; 6] {
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub parent: Option<NodeId>,
    pub visible: bool,
    pub locked: bool,
    pub locks: LayerLocks,
    pub color_label: LayerColor,
    /// Persistent movement relationship, independent of group hierarchy.
    pub link_group: Option<NodeId>,
    pub opacity: f32,
    pub blend: BlendMode,
    pub blending: emulsion_raster::composite::BlendingOptions,
    /// Clip to the content of a sibling below.
    pub clip_to: Option<NodeId>,
    /// Coverage mask. For Raster and Smart nodes it lives in source pixel space and
    /// moves with it; otherwise it is in document space.
    pub mask: Option<Arc<Mask>>,
    pub mask_enabled: bool,
    pub mask_linked: bool,
    /// Mask-source to layer-local affine, in DAffine2 column-array order.
    pub mask_transform: [f64; 6],
    /// Effects drawn from the node's alpha (shadows, glow, stroke, overlays).
    pub styles: Vec<crate::styles::LayerStyle>,
    /// Parallel per-effect controls; missing entries use defaults.
    pub style_options: Vec<crate::style_options::StyleOptions>,
    pub effects_enabled: bool,
    /// Provenance for content a model produced: `ai:<model id>`.
    pub origin: Option<String>,
    pub kind: NodeKind,
}

impl PartialEq for Node {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
            && self.name == o.name
            && self.parent == o.parent
            && self.visible == o.visible
            && self.locked == o.locked
            && self.locks == o.locks
            && self.color_label == o.color_label
            && self.link_group == o.link_group
            && self.opacity == o.opacity
            && self.blend == o.blend
            && self.blending == o.blending
            && self.clip_to == o.clip_to
            && match (&self.mask, &o.mask) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
            && self.mask_enabled == o.mask_enabled
            && self.mask_linked == o.mask_linked
            && self.mask_transform == o.mask_transform
            && self.styles == o.styles
            && self.style_options == o.style_options
            && self.effects_enabled == o.effects_enabled
            && self.origin == o.origin
            && self.kind == o.kind
    }
}

impl Node {
    pub fn new(id: NodeId, name: impl Into<String>, kind: NodeKind) -> Self {
        let blend = if kind.is_group() {
            BlendMode::PassThrough
        } else {
            BlendMode::Normal
        };
        Self {
            id,
            name: name.into(),
            parent: None,
            visible: true,
            locked: false,
            locks: LayerLocks::default(),
            color_label: LayerColor::None,
            link_group: None,
            opacity: 1.0,
            blend,
            blending: Default::default(),
            clip_to: None,
            mask: None,
            mask_enabled: true,
            mask_linked: true,
            mask_transform: default_mask_transform(),
            styles: Vec::new(),
            style_options: Vec::new(),
            effects_enabled: true,
            origin: None,
            kind,
        }
    }

    /// Mark the node as produced by a local model.
    pub fn from_model(mut self, model_id: &str) -> Self {
        self.origin = Some(format!("ai:{model_id}"));
        self
    }

    /// The model id when a model produced this node.
    pub fn model_id(&self) -> Option<&str> {
        self.origin.as_deref()?.strip_prefix("ai:")
    }

    pub fn raster(
        id: NodeId,
        name: impl Into<String>,
        raster: Arc<Raster>,
        placement: Placement,
    ) -> Self {
        Self::new(id, name, NodeKind::Raster { raster, placement })
    }

    /// A vector path node, rasterized for a `w × h` document.
    pub fn path(
        id: NodeId,
        name: impl Into<String>,
        path: Arc<Path>,
        style: PathStyle,
        w: u32,
        h: u32,
    ) -> Self {
        let style = style.sanitized();
        let cache = Arc::new(path.rasterize(&style, w, h));
        Self::new(id, name, NodeKind::Path { path, style, cache })
    }

    /// A text layer rasterized for a `w × h` document.
    pub fn text(
        id: NodeId,
        name: impl Into<String>,
        spec: crate::text::TextSpec,
        w: u32,
        h: u32,
    ) -> Self {
        let spec = spec.sanitized();
        let cache = Arc::new(crate::text::rasterize(&spec, w, h));
        Self::new(
            id,
            name,
            NodeKind::Text {
                spec: Arc::new(spec),
                cache,
            },
        )
    }

    /// A smart layer over `source` with `filters` applied.
    pub fn smart(
        id: NodeId,
        name: impl Into<String>,
        source: Arc<Raster>,
        filters: Vec<emulsion_filters::Filter>,
        placement: Placement,
    ) -> Self {
        let (cache, offset) = crate::smart::render(&source, &filters);
        Self::new(
            id,
            name,
            NodeKind::Smart {
                editable: None,
                source,
                filters,
                filter_styles: Vec::new(),
                placement,
                cache,
                offset,
            },
        )
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

/// Old native files show their layer effects by default.
pub fn default_effects_enabled() -> bool {
    true
}
