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
// Keep the bounded Copy paint inline to preserve the native PathStyle API and
// serde representation; at most sixteen stops per channel require no heap graph.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SmartEditable {
    /// Bounded native layered document. Native IO stores bytes in deduplicated ZIP resources.
    Document {
        archive: Arc<Vec<u8>>,
        external: Option<crate::smart_source::ExternalLink>,
    },
    /// Original SVG for resolution-independent placed artwork.
    Svg {
        xml: Arc<str>,
    },
    Text {
        spec: Arc<crate::text::TextSpec>,
    },
    Path {
        path: Arc<Path>,
        style: PathStyle,
    },
}

/// Immutable encoded PNG retained for byte-exact Smart Object interchange.
/// IO validates both digests before attaching this descriptor to its source.
/// Rendering continues to use the decoded `NodeKind::Smart::source` raster.
#[derive(Clone, Debug)]
pub struct OriginalImage {
    bytes: Arc<Vec<u8>>,
    encoded_sha256: [u8; 32],
    source_sha256: [u8; 32],
}

impl OriginalImage {
    pub fn new(bytes: Arc<Vec<u8>>, encoded_sha256: [u8; 32], source_sha256: [u8; 32]) -> Self {
        Self {
            bytes,
            encoded_sha256,
            source_sha256,
        }
    }

    pub fn bytes(&self) -> &Arc<Vec<u8>> {
        &self.bytes
    }

    pub fn encoded_sha256(&self) -> &[u8; 32] {
        &self.encoded_sha256
    }

    pub fn source_sha256(&self) -> &[u8; 32] {
        &self.source_sha256
    }
}

// Retained Smart metadata remains inline in the existing public node model;
// pixel/source payloads already share Arcs. Boxing a field here would change
// construction, matching and lifecycle APIs and add a heap allocation per Smart.
#[expect(
    clippy::large_enum_variant,
    reason = "keep authored Smart metadata inline without a lint-only node-model/API redesign"
)]
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
        cache: crate::vector_cache::VectorRaster,
    },
    /// A text layer, shaped and rasterized into `cache` (see `text`).
    Text {
        spec: Arc<crate::text::TextSpec>,
        cache: crate::vector_cache::VectorRaster,
    },
    /// Editable pencil strokes and fills in document space, rasterized into
    /// `cache` whenever they change (see `emulsion_raster::strokes`).
    Strokes {
        strokes: Arc<emulsion_raster::strokes::StrokeSet>,
        cache: crate::vector_cache::VectorRaster,
    },
    /// Source pixels with an editable filter stack, rendered into `cache`,
    /// whose top-left sits at `offset` in source pixels (see `smart`).
    Smart {
        editable: Option<SmartEditable>,
        source: Arc<Raster>,
        /// Retained encoded bytes, invalidated whenever the source pixels change.
        original_image: Option<Arc<OriginalImage>>,
        filters: Vec<emulsion_filters::Filter>,
        filter_styles: Vec<emulsion_filters::FilterStyle>,
        /// Root stack visibility; individual stage flags remain independent.
        filters_enabled: bool,
        filter_mask: Option<crate::SmartFilterMask>,
        placement: crate::SmartPlacement,
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
            NodeKind::Strokes { .. } => "vec",
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
            (NodeKind::Strokes { strokes: a, .. }, NodeKind::Strokes { strokes: b, .. }) => {
                Arc::ptr_eq(a, b) || a == b
            }
            (
                NodeKind::Smart {
                    source: a,
                    editable: ea,
                    original_image: oa,
                    filters: fa,
                    filter_styles: sa,
                    filters_enabled: ena,
                    filter_mask: ma,
                    placement: pa,
                    ..
                },
                NodeKind::Smart {
                    source: b,
                    editable: eb,
                    original_image: ob,
                    filters: fb,
                    filter_styles: sb,
                    filters_enabled: enb,
                    filter_mask: mb,
                    placement: pb,
                    ..
                },
            ) => {
                Arc::ptr_eq(a, b)
                    && ea == eb
                    && match (oa, ob) {
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    }
                    && fa == fb
                    && sa == sb
                    && ena == enb
                    && ma == mb
                    && pa == pb
            }
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
    /// Intrinsic coverage plane. Its independent, bounded source grid is mapped
    /// through mask_transform into layer-local space (source pixels for Raster/
    /// Smart, document pixels otherwise). Crop/resize retain original coverage.
    pub mask: Option<Arc<Mask>>,
    /// Editable geometry independent of the raster-mask component.
    pub vector_mask: Option<crate::VectorMask>,
    pub mask_enabled: bool,
    pub mask_properties: crate::MaskProperties,
    pub mask_linked: bool,
    /// Intrinsic mask to layer-local map, retained even with no raster plane.
    pub mask_transform: crate::Mapping2,
    /// Effects drawn from the node's alpha (shadows, glow, stroke, overlays).
    pub styles: Vec<crate::styles::LayerStyle>,
    /// Parallel per-effect controls; missing entries use defaults.
    pub style_options: Vec<crate::style_options::StyleOptions>,
    pub effects_enabled: bool,
    /// Provenance for content a model produced: `ai:<model id>`.
    pub origin: Option<String>,
    /// A storyboard review layer: drawn on the Stage, left out of every
    /// export (see `storyboard_review::printable`).
    pub review: bool,
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
            && self.vector_mask == o.vector_mask
            && self.mask_properties == o.mask_properties
            && self.mask_enabled == o.mask_enabled
            && self.mask_linked == o.mask_linked
            && self.mask_transform == o.mask_transform
            && self.styles == o.styles
            && self.style_options == o.style_options
            && self.effects_enabled == o.effects_enabled
            && self.origin == o.origin
            && self.review == o.review
            && self.kind == o.kind
    }
}

impl Node {
    pub fn has_mask(&self) -> bool {
        self.mask.is_some() || self.vector_mask.is_some()
    }
    pub fn has_enabled_mask(&self) -> bool {
        (self.mask.is_some() && self.mask_enabled && self.mask_properties.density > 0.0)
            || self
                .vector_mask
                .as_ref()
                .is_some_and(|m| m.enabled && m.properties.density > 0.0)
    }

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
            vector_mask: None,
            mask_enabled: true,
            mask_properties: Default::default(),
            mask_linked: true,
            mask_transform: crate::Mapping2::IDENTITY,
            styles: Vec::new(),
            style_options: Vec::new(),
            effects_enabled: true,
            origin: None,
            review: false,
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

    /// A vector stroke layer, rasterized for a `w × h` document.
    pub fn strokes(
        id: NodeId,
        name: impl Into<String>,
        strokes: Arc<emulsion_raster::strokes::StrokeSet>,
        w: u32,
        h: u32,
    ) -> Self {
        let cache = crate::vector_cache::VectorRaster::strokes(strokes.clone(), w, h);
        Self::new(id, name, NodeKind::Strokes { strokes, cache })
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
        let cache = crate::vector_cache::VectorRaster::path(path.clone(), style, w, h);
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
        let spec = Arc::new(spec.sanitized());
        let cache = crate::vector_cache::VectorRaster::text(spec.clone(), w, h);
        Self::new(id, name, NodeKind::Text { spec, cache })
    }

    /// A smart layer over `source` with `filters` applied.
    pub fn smart(
        id: NodeId,
        name: impl Into<String>,
        source: Arc<Raster>,
        filters: Vec<emulsion_filters::Filter>,
        placement: Placement,
    ) -> Self {
        let (cache, offset) = crate::smart::render_stack(&source, &filters, &[], true);
        Self::new(
            id,
            name,
            NodeKind::Smart {
                editable: None,
                original_image: None,
                filter_mask: None,
                source,
                filters,
                filter_styles: Vec::new(),
                filters_enabled: true,
                placement: crate::SmartPlacement::Legacy(placement),
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

    /// Variant presence, including latent raster and disabled filter metadata.
    pub fn projective_features(&self) -> crate::smart_support::ProjectiveFeatures {
        crate::smart_support::ProjectiveFeatures {
            placement: matches!(
                self.kind,
                NodeKind::Smart {
                    placement: crate::SmartPlacement::Projective(_),
                    ..
                }
            ),
            raster_mask: matches!(self.mask_transform, crate::Mapping2::Projective(_)),
            filter_mask: matches!(&self.kind, NodeKind::Smart { filter_mask: Some(mask), .. } if matches!(mask.transform, crate::Mapping2::Projective(_))),
        }
    }

    pub fn has_projective_metadata(&self) -> bool {
        self.projective_features().any()
    }

    pub fn require_affine_capability(
        &self,
        operation: &'static str,
    ) -> Result<(), crate::GeometryError> {
        if self.has_projective_metadata() {
            return Err(crate::GeometryError::retained_projective(operation));
        }
        Ok(())
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
