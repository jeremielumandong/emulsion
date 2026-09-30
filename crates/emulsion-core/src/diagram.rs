//! Structured diagrams on native editable groups, paths, and text.
use crate::{
    Command, Document, Editor, Node, NodeId, NodeKind,
    command::Slot,
    text::{Align, TextSpec},
};
use emulsion_raster::vector::{Anchor, Path, PathStyle, SubPath};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

#[path = "diagram_builder.rs"]
mod builder;
pub use builder::Builder;

#[path = "diagram_stencils.rs"]
pub mod stencils;

#[path = "diagram_layout.rs"]
mod layout;
#[path = "diagram_markers.rs"]
mod markers;
#[path = "diagram_router.rs"]
mod router;
pub use layout::{Layout, arrange};
pub use markers::{Marker, MarkerKind};
#[path = "diagram_jumps.rs"]
mod jumps;
pub use jumps::JumpStyle;
#[path = "diagram_decorations.rs"]
mod decorations;
#[path = "diagram_endpoints.rs"]
mod endpoints;
#[path = "diagram_label_position.rs"]
mod label_position;
#[path = "diagram_labels.rs"]
mod labels;
pub use endpoints::{connector_attachment, endpoint_position};
pub use label_position::{
    LabelColumn, LabelRow, caption_icon_labels, label_position, label_position_command,
};
pub use labels::EdgeLabel;
fn default_jump_size() -> f64 {
    10.
}

pub type Bounds = [f64; 4];
pub const DEFAULT_FILL: [u8; 4] = [255, 255, 255, 255];
pub const DEFAULT_LINE: [u8; 4] = [75, 81, 89, 255];
pub const DEFAULT_TEXT: [u8; 4] = [55, 61, 69, 255];
pub const DEFAULT_LINE_WIDTH: f32 = 1.;

pub const MAX_SHAPES: usize = 10_000;
pub const MAX_EDGES: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Process,
    Decision,
    Terminator,
    Data,
    Database,
    Document,
    Note,
    Class,
    Entity,
    Container,
    Swimlane,
    Cloud,
}
impl ShapeKind {
    pub const ALL: [Self; 12] = [
        Self::Process,
        Self::Decision,
        Self::Terminator,
        Self::Data,
        Self::Database,
        Self::Document,
        Self::Note,
        Self::Class,
        Self::Entity,
        Self::Container,
        Self::Swimlane,
        Self::Cloud,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Process => "Process",
            Self::Decision => "Decision",
            Self::Terminator => "Start / End",
            Self::Data => "Input / Output",
            Self::Database => "Database",
            Self::Document => "Document",
            Self::Note => "Note",
            Self::Class => "UML Class",
            Self::Entity => "Entity",
            Self::Container => "Container",
            Self::Swimlane => "Swimlane",
            Self::Cloud => "Cloud",
        }
    }
    pub fn is_container(self) -> bool {
        matches!(self, Self::Container | Self::Swimlane)
    }
    fn default_path(self, bounds: Bounds) -> Path {
        if self != Self::Process {
            return self.path(bounds);
        }
        let [x, y, w, h] = bounds;
        let r = 4_f64.min(w / 2.).min(h / 2.);
        Path::from_svg(&format!("M {} {y} H {} Q {} {y} {} {} V {} Q {} {} {} {} H {} Q {x} {} {x} {} V {} Q {x} {y} {} {y} Z",x+r,x+w-r,x+w,x+w,y+r,y+h-r,x+w,y+h,x+w-r,y+h,x+r,y+h,y+h-r,y+r,x+r)).expect("rounded process")
    }
    pub fn path(self, [x, y, w, h]: Bounds) -> Path {
        use crate::design::Element;
        let polygon = |points: &[(f64, f64)]| Path {
            subpaths: vec![SubPath {
                anchors: points
                    .iter()
                    .map(|(a, b)| Anchor::corner((x + a * w, y + b * h)))
                    .collect(),
                closed: true,
            }],
        };
        match self {
            Self::Decision=>Element::Diamond.path(x,y,w,h),
            Self::Terminator=>{
                let r=h.min(w)/2.;let k=0.5522847498*r;
                Path::from_svg(&format!("M {} {y} L {} {y} C {} {y} {} {} {} {} L {} {} C {} {} {} {} {} {} L {} {} C {} {} {x} {} {x} {} L {x} {} C {x} {} {} {y} {} {y} Z",x+r,x+w-r,x+w-r+k,x+w,y+r-k,x+w,y+r,x+w,y+h-r,x+w,y+h-r+k,x+w-r+k,y+h,x+w-r,y+h,x+r,y+h,x+r-k,y+h,y+h-r+k,y+h-r,y+r,y+r-k,x+r-k,x+r)).expect("finite rounded shape")
            },
            Self::Data=>polygon(&[(0.15,0.),(1.,0.),(0.85,1.),(0.,1.)]),
            Self::Note=>polygon(&[(0.,0.),(0.8,0.),(1.,0.2),(1.,1.),(0.,1.)]),
            Self::Document=>polygon(&[(0.,0.),(1.,0.),(1.,0.88),(0.75,0.8),(0.25,1.),(0.,0.9)]),
            Self::Database=>Path::from_svg(&format!("M {x} {} C {x} {y} {} {y} {} {} L {} {} C {} {} {x} {} {x} {} Z M {x} {} C {x} {} {} {} {} {}",y+h*0.15,x+w,x+w,y+h*0.15,x+w,y+h*0.85,x+w,y+h*1.05,y+h*1.05,y+h*0.85,y+h*0.15,y+h*0.35,x+w,y+h*0.35,x+w,y+h*0.15)).unwrap_or_else(|_|Element::Rectangle.path(x,y,w,h)),
            Self::Cloud=>{
                let mut p=Path::from_svg("M 0.22 0.85 C -0.04 0.88 -0.06 0.38 0.18 0.34 C 0.12 0.05 0.48 -0.04 0.59 0.18 C 0.76 -0.04 1.04 0.20 0.89 0.40 C 1.13 0.58 0.94 0.96 0.74 0.84 C 0.63 1.05 0.30 1.05 0.22 0.85 Z").expect("cloud path");
                p.transform(glam::DAffine2::from_cols_array(&[w,0.,0.,h,x,y]));
                p
            },
            _=>Element::Rectangle.path(x,y,w,h),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub body: NodeId,
    pub label: NodeId,
    pub kind: ShapeKind,
    #[serde(default)]
    pub container: Option<NodeId>,
    #[serde(default)]
    pub data: BTreeMap<String, String>,
    #[serde(default)]
    pub layout_locked: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<ConditionalFill>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unconditional_style: Option<PathStyle>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConditionalFill {
    pub field: String,
    pub equals: String,
    pub color: [u8; 4],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Port {
    #[default]
    Auto,
    North,
    East,
    South,
    West,
    Custom {
        x: f64,
        y: f64,
    },
}
impl Port {
    pub const ALL: [Self; 5] = [Self::Auto, Self::North, Self::East, Self::South, Self::West];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::North => "North",
            Self::East => "East",
            Self::South => "South",
            Self::West => "West",
            Self::Custom { .. } => "Custom",
        }
    }
    fn valid(self) -> bool {
        match self {
            Self::Custom { x, y } => {
                // Imported connection points may lie outside the shape perimeter.
                x.is_finite() && y.is_finite() && x.abs() <= 100. && y.abs() <= 100.
            }
            _ => true,
        }
    }
    pub fn anchor(self, [x, y, w, h]: Bounds, toward: (f64, f64)) -> ((f64, f64), (f64, f64)) {
        let port = if self == Self::Auto {
            let dx = toward.0 - (x + w / 2.);
            let dy = toward.1 - (y + h / 2.);
            if dx.abs() / w > dy.abs() / h {
                if dx >= 0. { Self::East } else { Self::West }
            } else if dy >= 0. {
                Self::South
            } else {
                Self::North
            }
        } else {
            self
        };
        match port {
            Self::North => ((x + w / 2., y), (0., -1.)),
            Self::East => ((x + w, y + h / 2.), (1., 0.)),
            Self::South => ((x + w / 2., y + h), (0., 1.)),
            Self::West => ((x, y + h / 2.), (-1., 0.)),
            Self::Custom { x: a, y: b } => {
                let dx = a - 0.5;
                let dy = b - 0.5;
                (
                    (x + a * w, y + b * h),
                    if dx.abs() > dy.abs() {
                        (dx.signum(), 0.)
                    } else {
                        (0., if dy == 0. { 1. } else { dy.signum() })
                    },
                )
            }
            Self::Auto => unreachable!(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    pub shape: NodeId,
    pub port: Port,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Routing {
    Straight,
    Curved,
    Cyclical,
    #[default]
    Orthogonal,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// Automatic routing could not find a clear corridor; use manual waypoints or move overlapping objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_warning: Option<String>,
    #[serde(default)]
    pub double_line: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_background: Option<[u8; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub double_path: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_background_path: Option<NodeId>,
    #[serde(default)]
    pub corner_radius: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<EdgeLabel>,
    #[serde(default)]
    pub jump_style: JumpStyle,
    #[serde(default = "default_jump_size")]
    pub jump_size: f64,
    pub path: NodeId,
    pub arrow: NodeId,
    pub label: NodeId,
    pub source: Endpoint,
    pub target: Endpoint,
    #[serde(default)]
    pub routing: Routing,
    #[serde(default)]
    pub waypoints: Vec<(f64, f64)>,
    #[serde(default)]
    pub label_offset: (f64, f64),
    /// Relative position along the route: -1 source, 0 center, 1 target.
    #[serde(default)]
    pub label_position: f64,
    /// Perpendicular displacement from the route, in document units.
    #[serde(default)]
    pub label_normal: f64,
    #[serde(default)]
    pub start_marker: Marker,
    #[serde(default)]
    pub end_marker: Marker,
    pub arrow_end: bool,
    #[serde(default)]
    pub arrow_start: bool,
}
impl Edge {
    pub fn reverse(&mut self) {
        std::mem::swap(&mut self.source, &mut self.target);
        self.waypoints.reverse();
        self.label_position = -self.label_position;
        self.label_normal = -self.label_normal;
        for label in &mut self.labels {
            label.position = -label.position;
            label.normal = -label.normal;
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagram {
    pub shapes: BTreeMap<NodeId, Shape>,
    pub edges: BTreeMap<NodeId, Edge>,
    #[serde(default)]
    pub settings: workspace::Settings,
}

pub fn shape_bounds(doc: &Document, shape: &Shape) -> Option<Bounds> {
    if let NodeKind::Path { path, .. } = &doc.node(shape.body)?.kind {
        let (x, y, w, h) = emulsion_raster::vector_geometry::bounds(path)?;
        Some([x, y, w.max(1.), h.max(1.)])
    } else {
        let b = crate::geometry::node_bounds(doc, shape.body)?;
        Some([b.x as f64, b.y as f64, b.w.max(1) as f64, b.h.max(1) as f64])
    }
}
fn center([x, y, w, h]: Bounds) -> (f64, f64) {
    (x + w / 2., y + h / 2.)
}
fn valid_bounds(b: Bounds) -> bool {
    b.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.) && b[2] >= 1. && b[3] >= 1.
}

impl Diagram {
    pub fn validate(&self, doc: &Document) -> Result<(), String> {
        self.settings.validate(self, doc)?;
        if self.shapes.len() > MAX_SHAPES || self.edges.len() > MAX_EDGES {
            return Err("Diagram exceeds the shape or connector limit.".into());
        }
        let nodes = doc
            .nodes
            .iter()
            .map(|n| (n.id, n))
            .collect::<HashMap<_, _>>();
        let node = |id| nodes.get(&id).copied();
        let ancestor = |parent, mut child| {
            for _ in 0..=nodes.len() {
                let Some(next) = node(child).and_then(|n| n.parent) else {
                    return false;
                };
                if next == parent {
                    return true;
                }
                child = next;
            }
            false
        };
        let mut owned = HashSet::new();
        for (id, shape) in &self.shapes {
            structure::validate(shape)?;
            if !node(*id).is_some_and(Node::is_group)
                || !owned.insert(*id)
                || !owned.insert(shape.body)
                || !owned.insert(shape.label)
                || !ancestor(*id, shape.body)
                || !ancestor(*id, shape.label)
                || !node(shape.label).is_some_and(|n| matches!(n.kind, NodeKind::Text { .. }))
                || node(shape.body)
                    .and_then(|n| {
                        if let NodeKind::Path { path, .. } = &n.kind {
                            emulsion_raster::vector_geometry::bounds(path)
                                .map(|(x, y, w, h)| [x, y, w.max(1.), h.max(1.)])
                        } else {
                            None
                        }
                    })
                    .is_none_or(|b| !valid_bounds(b))
            {
                return Err("Invalid diagram shape references or bounds.".into());
            }
            if shape.data.len() > 64
                || shape
                    .data
                    .iter()
                    .any(|(k, v)| k.is_empty() || k.len() > 128 || v.len() > 4096)
            {
                return Err("Shape data exceeds its limits.".into());
            }
            if shape.conditions.len() > 32
                || shape
                    .conditions
                    .iter()
                    .any(|r| r.field.is_empty() || r.field.len() > 128 || r.equals.len() > 4096)
                || shape
                    .unconditional_style
                    .is_some_and(|s| s.sanitized() != s)
            {
                return Err("Invalid conditional shape style.".into());
            }
            let mut seen = HashSet::from([*id]);
            let mut parent = shape.container;
            while let Some(id) = parent {
                if !seen.insert(id) {
                    return Err("Diagram containers form a cycle.".into());
                }
                let container = self
                    .shapes
                    .get(&id)
                    .filter(|s| s.kind.is_container())
                    .ok_or("Missing diagram container")?;
                parent = container.container;
            }
        }
        for (id, edge) in &self.edges {
            if !node(*id).is_some_and(Node::is_group)
                || !owned.insert(*id)
                || !owned.insert(edge.path)
                || !owned.insert(edge.arrow)
                || !owned.insert(edge.label)
                || [edge.double_path, edge.label_background_path]
                    .into_iter()
                    .flatten()
                    .any(|child| {
                        !owned.insert(child)
                            || !ancestor(*id, child)
                            || !node(child).is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
                    })
                || !self.contains_endpoint(edge.source.shape)
                || !self.contains_endpoint(edge.target.shape)
                || !edge.source.port.valid()
                || !edge.target.port.valid()
                || !edge.start_marker.valid()
                || !edge.end_marker.valid()
                || !edge.corner_radius.is_finite()
                || !(0. ..=100.).contains(&edge.corner_radius)
                || !edge.jump_size.is_finite()
                || !(1. ..=100.).contains(&edge.jump_size)
                || edge.labels.len() > 128
                || edge.labels.iter().any(|l| {
                    !owned.insert(l.node)
                        || !ancestor(*id, l.node)
                        || !node(l.node).is_some_and(|n| matches!(n.kind, NodeKind::Text { .. }))
                        || !l.position.is_finite()
                        || !(-1. ..=1.).contains(&l.position)
                        || [l.normal, l.offset.0, l.offset.1]
                            .iter()
                            .any(|v| !v.is_finite() || v.abs() > 1e6)
                })
                || !edge.label_position.is_finite()
                || !(-1. ..=1.).contains(&edge.label_position)
                || !edge.label_normal.is_finite()
                || edge.label_normal.abs() > 1e6
                || edge.waypoints.len() > 128
                || edge
                    .waypoints
                    .iter()
                    .chain(std::iter::once(&edge.label_offset))
                    .any(|(x, y)| {
                        !x.is_finite()
                            || !y.is_finite()
                            || x.abs() > 1_000_000.
                            || y.abs() > 1_000_000.
                    })
                || [edge.path, edge.arrow, edge.label]
                    .iter()
                    .any(|child| !ancestor(*id, *child))
                || !node(edge.path).is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
                || !node(edge.arrow).is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
                || !node(edge.label).is_some_and(|n| matches!(n.kind, NodeKind::Text { .. }))
            {
                return Err("Invalid diagram connector references or geometry.".into());
            }
        }
        self.edge_order()?;
        Ok(())
    }

    pub fn remap(&self, map: &HashMap<NodeId, NodeId>) -> Self {
        let id = |old| map.get(&old).copied().unwrap_or(old);
        Self {
            settings: self.settings.remap(map),
            shapes: self
                .shapes
                .iter()
                .map(|(key, s)| {
                    let mut s = s.clone();
                    s.body = id(s.body);
                    s.label = id(s.label);
                    s.container = s.container.map(id);
                    (id(*key), s)
                })
                .collect(),
            edges: self
                .edges
                .iter()
                .map(|(key, e)| {
                    let mut e = e.clone();
                    e.path = id(e.path);
                    e.arrow = id(e.arrow);
                    e.label = id(e.label);
                    e.double_path = e.double_path.map(id);
                    e.label_background_path = e.label_background_path.map(id);
                    for label in &mut e.labels {
                        label.node = id(label.node);
                    }
                    e.source.shape = id(e.source.shape);
                    e.target.shape = id(e.target.shape);
                    (id(*key), e)
                })
                .collect(),
        }
    }
    pub fn fragment(&self, ids: &HashSet<NodeId>) -> Self {
        Self {
            settings: self.settings.fragment(ids),
            shapes: self
                .shapes
                .iter()
                .filter(|(id, s)| {
                    ids.contains(id) && ids.contains(&s.body) && ids.contains(&s.label)
                })
                .map(|(id, s)| {
                    let mut s = s.clone();
                    if s.container.is_some_and(|id| !ids.contains(&id)) {
                        s.container = None;
                    }
                    (*id, s)
                })
                .collect(),
            edges: self
                .edges
                .iter()
                .filter(|(id, e)| {
                    ids.contains(id)
                        && ids.contains(&e.source.shape)
                        && ids.contains(&e.target.shape)
                        && [e.path, e.arrow, e.label].iter().all(|id| ids.contains(id))
                })
                .map(|(id, e)| (*id, e.clone()))
                .collect(),
        }
    }
}

fn set_path(doc: &mut Document, index: usize, path: Path) {
    let (w, h) = (doc.width, doc.height);
    if let Some(node) = doc.nodes.get_mut(index)
        && let NodeKind::Path {
            path: old,
            style,
            cache,
        } = &mut node.kind
        && **old != path
    {
        *old = Arc::new(path);
        *cache = crate::vector_cache::VectorRaster::path(old.clone(), *style, w, h);
    }
}

/// Called inside the same native command transaction. Moving/deleting shapes
/// updates dependent connectors before validation and before the undo snapshot.
pub fn synchronize(before: &Document, doc: &mut Document) -> Result<(), String> {
    let Some(model) = doc.diagram.as_ref() else {
        return Ok(());
    };
    let mut diagram = (**model).clone();
    let mut indices = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect::<HashMap<_, _>>();
    let before_nodes = before
        .nodes
        .iter()
        .map(|n| (n.id, n))
        .collect::<HashMap<_, _>>();
    let ancestor = |parent, mut child| {
        for _ in 0..=crate::document::MAX_DEPTH {
            let Some(next) = indices.get(&child).and_then(|i| doc.nodes[*i].parent) else {
                return false;
            };
            if next == parent {
                return true;
            }
            child = next;
        }
        false
    };
    diagram.shapes.retain(|id, s| {
        indices.get(id).is_some_and(|i| doc.nodes[*i].is_group())
            && indices
                .get(&s.body)
                .map(|i| &doc.nodes[*i])
                .is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
            && indices
                .get(&s.label)
                .map(|i| &doc.nodes[*i])
                .is_some_and(|n| matches!(n.kind, NodeKind::Text { .. }))
            && ancestor(*id, s.body)
            && ancestor(*id, s.label)
    });
    let containers: HashSet<_> = diagram
        .shapes
        .iter()
        .filter(|(_, s)| s.kind.is_container())
        .map(|(id, _)| *id)
        .collect();
    for (id, shape) in &mut diagram.shapes {
        shape.container = indices
            .get(id)
            .map(|i| &doc.nodes[*i])
            .and_then(|n| n.parent)
            .filter(|id| containers.contains(id));
    }
    let mut gone = HashSet::new();
    loop {
        let existing = diagram
            .shapes
            .keys()
            .chain(diagram.edges.keys())
            .copied()
            .collect::<HashSet<_>>();
        let count = diagram.edges.len();
        diagram.edges.retain(|id, e| {
            let keep = indices.contains_key(id)
                && [e.path, e.arrow, e.label]
                    .iter()
                    .all(|id| indices.contains_key(id))
                && existing.contains(&e.source.shape)
                && existing.contains(&e.target.shape);
            if !keep {
                gone.extend(doc.subtree(*id));
            }
            keep
        });
        if diagram.edges.len() == count {
            break;
        }
    }
    if !gone.is_empty() {
        doc.nodes.retain(|n| !gone.contains(&n.id));
        for n in &mut doc.nodes {
            if n.clip_to.is_some_and(|id| gone.contains(&id)) {
                n.clip_to = None;
            }
        }
    }
    if !gone.is_empty() {
        indices = doc
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
    }
    for edge in diagram.edges.values_mut() {
        edge.labels.retain(|l| indices.contains_key(&l.node));
    }
    if !diagram.settings.thumbnail.is_empty() || !diagram.settings.threads.is_empty() {
        diagram.settings.retain(&indices.keys().copied().collect());
        // A body can be converted/deleted while its group remains. Review
        // threads follow graph objects, so discard references to retired ones.
        diagram.settings.threads.retain(|_, thread| {
            diagram.shapes.contains_key(&thread.object)
                || diagram.edges.contains_key(&thread.object)
        });
    }
    diagram.validate(doc)?;
    let bounds = |node: &Node| {
        let NodeKind::Path { path, .. } = &node.kind else {
            return None;
        };
        emulsion_raster::vector_geometry::bounds(path)
            .map(|(x, y, w, h)| [x, y, w.max(1.), h.max(1.)])
    };
    for shape in diagram.shapes.values_mut() {
        if shape.conditions.is_empty() && shape.unconditional_style.is_none() {
            continue;
        }
        let (w, h) = (doc.width, doc.height);
        let old = before_nodes.get(&shape.body).copied().and_then(|n| {
            if let NodeKind::Path { style, .. } = &n.kind {
                Some(*style)
            } else {
                None
            }
        });
        if let Some(Node {
            kind: NodeKind::Path { path, style, cache },
            ..
        }) = indices.get(&shape.body).map(|i| &mut doc.nodes[*i])
        {
            let mut base = shape.unconditional_style.unwrap_or(*style);
            if let Some(old) = old
                && *style != old
            {
                let fill = base.fill;
                let paint = base.fill_paint;
                base = *style;
                if style.fill == old.fill {
                    base.fill = fill;
                }
                if style.fill_paint == old.fill_paint {
                    base.fill_paint = paint;
                }
            }
            let mut next = base;
            for rule in &shape.conditions {
                if shape.data.get(&rule.field) == Some(&rule.equals) {
                    next.fill = Some(rule.color);
                    next.fill_paint = emulsion_raster::vector::PathPaint::Solid;
                }
            }
            shape.unconditional_style = (!shape.conditions.is_empty()).then_some(base);
            if next != *style {
                *style = next;
                *cache = crate::vector_cache::VectorRaster::path(path.clone(), next, w, h);
            }
        }
    }
    structure::synchronize(before, doc, &mut diagram)?;
    let new_bounds = diagram
        .shapes
        .iter()
        .filter_map(|(id, s)| {
            indices
                .get(&s.body)
                .and_then(|i| bounds(&doc.nodes[*i]))
                .map(|b| (*id, b))
        })
        .collect::<HashMap<_, _>>();
    let obstacles = diagram
        .shapes
        .iter()
        .filter(|(_, s)| !s.kind.is_container())
        .filter_map(|(id, _)| new_bounds.get(id).copied())
        .collect::<Vec<_>>();
    let old_bounds = before.diagram.as_ref().into_iter().flat_map(|d|&d.shapes).filter_map(|(id,s)|{
        let old=before_nodes.get(&s.body)?;
        let unchanged=indices.get(&s.body).is_some_and(|i|matches!((&old.kind,&doc.nodes[*i].kind),(NodeKind::Path{path:a,..},NodeKind::Path{path:b,..}) if Arc::ptr_eq(a,b)));
        (if unchanged{new_bounds.get(id).copied()}else{bounds(old)}).map(|b|(*id,b))
    }).collect::<HashMap<_,_>>();
    let changed_bounds = new_bounds
        .iter()
        .filter(|(id, b)| old_bounds.get(id) != Some(*b))
        .map(|(_, b)| *b)
        .collect::<Vec<_>>();
    let (doc_width, doc_height) = (doc.width, doc.height);
    let edge_order = diagram.edge_order()?;
    let mut edge_paths = diagram
        .edges
        .iter()
        .filter_map(|(id, e)| match &doc.nodes[indices[&e.path]].kind {
            NodeKind::Path { path, .. } => Some((*id, path.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    for id in &edge_order {
        let edge = diagram.edges.get_mut(id).unwrap();
        let line_color = indices
            .get(&edge.path)
            .map(|i| &doc.nodes[*i])
            .and_then(|n| match &n.kind {
                NodeKind::Path { style, .. } => Some(style.stroke),
                _ => None,
            })
            .flatten();
        if let Some(node) = indices.get(&edge.arrow).map(|i| &mut doc.nodes[*i])
            && let NodeKind::Path { path, style, cache } = &mut node.kind
            && (style.fill != line_color || style.stroke.is_some() || style.dash_count != 0)
        {
            style.fill = line_color;
            style.stroke = None;
            style.dash_count = 0;
            style.dash = [0.; 6];
            *cache = crate::vector_cache::VectorRaster::path(
                path.clone(),
                *style,
                doc_width,
                doc_height,
            );
        }
        if !edge_paths.contains_key(&edge.source.shape)
            && !edge_paths.contains_key(&edge.target.shape)
            && edge.jump_style == JumpStyle::None
            && before.diagram.as_ref().and_then(|d| d.edges.get(id)) == Some(edge)
            && old_bounds.get(&edge.source.shape) == new_bounds.get(&edge.source.shape)
            && old_bounds.get(&edge.target.shape) == new_bounds.get(&edge.target.shape)
            && let (Some(old_path), Some(new_path), Some(old_label), Some(new_label)) = (
                before_nodes.get(&edge.path),
                indices.get(&edge.path).map(|i| &doc.nodes[*i]),
                before_nodes.get(&edge.label),
                indices.get(&edge.label).map(|i| &doc.nodes[*i]),
            )
            && let (
                NodeKind::Path {
                    path: old,
                    style: old_style,
                    ..
                },
                NodeKind::Path {
                    path: new,
                    style: new_style,
                    ..
                },
                NodeKind::Text { spec: old_text, .. },
                NodeKind::Text { spec: new_text, .. },
            ) = (
                &old_path.kind,
                &new_path.kind,
                &old_label.kind,
                &new_label.kind,
            )
            && (Arc::ptr_eq(old, new) || old == new)
            && old_style.width == new_style.width
            && old_text.x == new_text.x
            && old_text.y == new_text.y
        {
            let route_bounds = emulsion_raster::vector_geometry::bounds(old);
            let intersects = route_bounds.is_some_and(|(x, y, w, h)| {
                changed_bounds.iter().any(|b| {
                    x - 20. < b[0] + b[2]
                        && x + w + 20. > b[0]
                        && y - 20. < b[1] + b[3]
                        && y + h + 20. > b[1]
                })
            });
            if edge.routing != Routing::Orthogonal || !edge.waypoints.is_empty() || !intersects {
                continue;
            }
        }
        // Native transforms move the editable path too. Retain its transformed
        // interior anchors as manual waypoints before rebuilding bound endpoints.
        if !edge.waypoints.is_empty()
            && edge.jump_style == JumpStyle::None
            && edge.corner_radius == 0.
            && before
                .diagram
                .as_ref()
                .and_then(|d| d.edges.get(id))
                .is_none_or(|e| e.jump_style == JumpStyle::None && e.corner_radius == 0.)
            && let (Some(old), Some(new)) = (
                before_nodes.get(&edge.path).copied(),
                indices.get(&edge.path).map(|i| &doc.nodes[*i]),
            )
            && let (NodeKind::Path { path: old, .. }, NodeKind::Path { path: new, .. }) =
                (&old.kind, &new.kind)
            && old != new
            && let Some(line) = new.subpaths.first()
            && (2..=130).contains(&line.anchors.len())
        {
            edge.waypoints = line.anchors[1..line.anchors.len() - 1]
                .iter()
                .map(|a| a.p)
                .collect();
        }
        let (start, end, mut route) = route_geometry(edge, &new_bounds, &edge_paths, &obstacles)?;
        let middle = labels::point(&route, edge.label_position, edge.label_normal);
        if let (Some(old), Some(new)) = (
            before_nodes.get(&edge.label).copied(),
            indices.get(&edge.label).map(|i| &doc.nodes[*i]),
        ) && let (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) =
            (&old.kind, &new.kind)
            && (a.x != b.x || a.y != b.y)
        {
            edge.label_offset = (b.x as f64 + 60. - middle.0, b.y as f64 + 22. - middle.1);
        }

        let tangent = route.subpaths.first().unwrap();
        let start_prior = if tangent.anchors[0].h_out != tangent.anchors[0].p {
            tangent.anchors[0].h_out
        } else {
            tangent.anchors[1].p
        };
        let last = tangent.anchors.last().unwrap();
        let end_prior = if last.h_in != last.p {
            last.h_in
        } else {
            tangent.anchors[tangent.anchors.len() - 2].p
        };
        // Stop the connector beneath closed markers, keeping hollow interiors clear.
        let anchors = &mut route.subpaths[0].anchors;
        for (at, tip, prior, marker, enabled) in [
            (0, start, start_prior, edge.start_marker, edge.arrow_start),
            (
                anchors.len() - 1,
                end,
                end_prior,
                edge.end_marker,
                edge.arrow_end,
            ),
        ] {
            if enabled {
                let (dx, dy) = (prior.0 - tip.0, prior.1 - tip.1);
                let length = dx.hypot(dy);
                if length > 1e-6 {
                    let distance = marker.inset().min(length * 0.45);
                    let delta = (dx / length * distance, dy / length * distance);
                    anchors[at].p = (tip.0 + delta.0, tip.1 + delta.1);
                    anchors[at].h_in = (anchors[at].h_in.0 + delta.0, anchors[at].h_in.1 + delta.1);
                    anchors[at].h_out =
                        (anchors[at].h_out.0 + delta.0, anchors[at].h_out.1 + delta.1);
                }
            }
        }
        edge_paths.insert(*id, Arc::new(route.clone()));
        set_path(doc, indices[&edge.path], route);
        let width = match &doc.node(edge.path).unwrap().kind {
            NodeKind::Path { style, .. } => style.width as f64,
            _ => 1.,
        };
        let mut arrow = Path::default();
        for (enabled, marker, tip, prior) in [
            (edge.arrow_end, edge.end_marker, end, end_prior),
            (edge.arrow_start, edge.start_marker, start, start_prior),
        ] {
            if enabled {
                arrow
                    .subpaths
                    .extend(marker.path(tip, prior, width).subpaths);
            }
        }
        set_path(doc, indices[&edge.arrow], arrow);
        let (w, h) = (doc.width, doc.height);
        if let Some(node) = indices.get(&edge.label).map(|i| &mut doc.nodes[*i])
            && let NodeKind::Text { spec, cache } = &mut node.kind
        {
            let mut next = (**spec).clone();
            next.x = (middle.0 - 60. + edge.label_offset.0) as f32;
            next.y = (middle.1 - 22. + edge.label_offset.1) as f32;
            if next != **spec {
                *spec = Arc::new(next);
                *cache = crate::vector_cache::VectorRaster::text(spec.clone(), w, h);
            }
        }
    }
    labels::synchronize(before, doc, &mut diagram);
    jumps::apply(doc, &diagram);
    decorations::synchronize(before, doc, &mut diagram);
    doc.diagram = Some(Arc::new(diagram));
    Ok(())
}

/// Immutable routing context captured once per drag; each preview routes only
/// the edited connector and never rebuilds document or raster caches.
pub struct ConnectorPreview {
    bounds: HashMap<NodeId, Bounds>,
    paths: HashMap<NodeId, Arc<Path>>,
    obstacles: Vec<Bounds>,
}
impl ConnectorPreview {
    pub fn new(doc: &Document) -> Option<Self> {
        let diagram = doc.diagram.as_ref()?;
        let nodes = doc
            .nodes
            .iter()
            .map(|n| (n.id, n))
            .collect::<HashMap<_, _>>();
        let shape_bounds = diagram
            .shapes
            .iter()
            .filter_map(|(id, s)| {
                let NodeKind::Path { path, .. } = &nodes.get(&s.body)?.kind else {
                    return None;
                };
                emulsion_raster::vector_geometry::bounds(path)
                    .map(|(x, y, w, h)| (*id, [x, y, w.max(1.), h.max(1.)]))
            })
            .collect::<HashMap<_, _>>();
        let paths = diagram
            .edges
            .iter()
            .filter_map(|(id, e)| match &nodes.get(&e.path)?.kind {
                NodeKind::Path { path, .. } => Some((*id, path.clone())),
                _ => None,
            })
            .collect();
        let obstacles = diagram
            .shapes
            .iter()
            .filter(|(_, s)| !s.kind.is_container())
            .filter_map(|(id, _)| shape_bounds.get(id).copied())
            .collect();
        Some(Self {
            bounds: shape_bounds,
            paths,
            obstacles,
        })
    }
    /// Includes rounded corners and curves. Arrow insets and crossing decorations
    /// are omitted so attachment handles stay at the actual connection point.
    pub fn path(&self, edge: &Edge) -> Option<Path> {
        route_geometry(
            &mut edge.clone(),
            &self.bounds,
            &self.paths,
            &self.obstacles,
        )
        .ok()
        .map(|(_, _, path)| path)
    }
}

#[allow(clippy::type_complexity)]
fn route_geometry(
    edge: &mut Edge,
    new_bounds: &HashMap<NodeId, Bounds>,
    edge_paths: &HashMap<NodeId, Arc<Path>>,
    obstacles: &[Bounds],
) -> Result<((f64, f64), (f64, f64), Path), String> {
    let source_on_edge = edge_paths
        .get(&edge.source.shape)
        .map(|p| endpoints::on_path(p, edge.source.port));
    let target_on_edge = edge_paths
        .get(&edge.target.shape)
        .map(|p| endpoints::on_path(p, edge.target.port));
    let bounds_for = |endpoint: &Endpoint, point: Option<((f64, f64), (f64, f64))>| {
        point
            .map(|(p, _)| [p.0, p.1, 0., 0.])
            .or_else(|| new_bounds.get(&endpoint.shape).copied())
            .ok_or("Missing connector endpoint bounds")
    };
    let a = bounds_for(&edge.source, source_on_edge)?;
    let b = bounds_for(&edge.target, target_on_edge)?;
    let self_loop = edge.source.shape == edge.target.shape;
    let source_port = if self_loop && edge.source.port == Port::Auto {
        Port::East
    } else {
        edge.source.port
    };
    let target_port = if self_loop && edge.target.port == Port::Auto {
        Port::North
    } else {
        edge.target.port
    };
    let (start, sd) = source_on_edge.unwrap_or_else(|| source_port.anchor(a, center(b)));
    let (end, ed) = target_on_edge.unwrap_or_else(|| target_port.anchor(b, center(a)));
    edge.routing_warning = None;
    let points = if !edge.waypoints.is_empty() {
        std::iter::once(start)
            .chain(edge.waypoints.iter().copied())
            .chain(std::iter::once(end))
            .collect()
    } else if edge.routing == Routing::Cyclical {
        router::cyclical(start, sd, end, ed, a, b)
    } else if edge.routing != Routing::Orthogonal {
        vec![start, end]
    } else {
        let (points, blocked) = router::orthogonal(start, sd, end, ed, obstacles);
        if blocked {
            edge.routing_warning = Some(
                "No clear automatic route. Move overlapping objects or add manual waypoints."
                    .into(),
            );
        }
        points
    };

    let route = if matches!(edge.routing, Routing::Curved | Routing::Cyclical) {
        markers::curved(&points)
    } else if edge.corner_radius > 0. {
        markers::rounded(&points, edge.corner_radius)
    } else {
        Path {
            subpaths: vec![SubPath {
                anchors: points.iter().copied().map(Anchor::corner).collect(),
                closed: false,
            }],
        }
    };
    Ok((start, end, route))
}

pub fn add_shape(
    editor: &mut Editor,
    kind: ShapeKind,
    bounds: Bounds,
    label: &str,
) -> Result<NodeId, String> {
    add_shape_inner(editor, kind, bounds, label, true)
}

fn add_shape_inner(
    editor: &mut Editor,
    kind: ShapeKind,
    bounds: Bounds,
    label: &str,
    own_transaction: bool,
) -> Result<NodeId, String> {
    if !valid_bounds(bounds) || label.chars().count() > crate::text::MAX_CHARS {
        return Err("Invalid shape size or label.".into());
    }
    if own_transaction && editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    if editor
        .doc
        .diagram
        .as_ref()
        .is_some_and(|d| d.shapes.len() >= MAX_SHAPES)
    {
        return Err("Diagram shape limit reached.".into());
    }
    if own_transaction {
        editor.begin("Add diagram shape");
    }
    let result = (|| {
        let group = editor
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    kind.label(),
                    NodeKind::Group { collapsed: false },
                )),
                slot: Slot::TOP,
            })
            .map_err(|e| e.to_string())?
            .unwrap();
        let color = if kind == ShapeKind::Note {
            [255, 237, 166, 255]
        } else if kind.is_container() {
            [241, 244, 249, 255]
        } else {
            DEFAULT_FILL
        };
        let body = editor
            .execute(Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    "Shape",
                    Arc::new(kind.default_path(bounds)),
                    PathStyle {
                        fill: Some(color),
                        stroke: Some(DEFAULT_LINE),
                        width: DEFAULT_LINE_WIDTH,
                        ..Default::default()
                    },
                    editor.doc.width,
                    editor.doc.height,
                )),
                slot: Slot::top_of(Some(group)),
            })
            .map_err(|e| e.to_string())?
            .unwrap();
        let [x, y, w, h] = bounds;
        let text = TextSpec {
            text: label.into(),
            font: "Geist".into(),
            size: 14.,
            x: (x + 8.) as f32,
            y: (y
                + if kind.is_container() || matches!(kind, ShapeKind::Class | ShapeKind::Entity) {
                    8.
                } else {
                    (h - 20.) / 2.
                }) as f32,
            width: Some((w - 16.).max(1.) as f32),
            align: Align::Center,
            color: DEFAULT_TEXT,
            ..Default::default()
        };
        let structure_data = if matches!(kind, ShapeKind::Class | ShapeKind::Entity) {
            BTreeMap::from([(
                "emulsion_structure".into(),
                serde_json::to_string(&structure::StructuredObject::from_text(label))
                    .map_err(|e| e.to_string())?,
            )])
        } else {
            BTreeMap::new()
        };
        let label = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Label",
                    text,
                    editor.doc.width,
                    editor.doc.height,
                )),
                slot: Slot::top_of(Some(group)),
            })
            .map_err(|e| e.to_string())?
            .unwrap();
        let mut diagram = editor.doc.diagram.as_deref().cloned().unwrap_or_default();
        diagram.shapes.insert(
            group,
            Shape {
                body,
                label,
                kind,
                container: None,
                data: structure_data,
                layout_locked: false,
                conditions: Vec::new(),
                unconditional_style: None,
            },
        );
        editor
            .execute(Command::SetDiagram {
                diagram: Some(Arc::new(diagram)),
            })
            .map_err(|e| e.to_string())?;
        workspace::apply_default(editor, group, false)?;
        Ok(group)
    })();
    match result {
        Ok(id) => {
            if own_transaction {
                editor.end();
            }
            Ok(id)
        }
        Err(error) => {
            if own_transaction {
                editor.cancel();
            }
            Err(error)
        }
    }
}

pub fn connect(
    editor: &mut Editor,
    source: Endpoint,
    target: Endpoint,
    label: &str,
    routing: Routing,
) -> Result<NodeId, String> {
    connect_inner(editor, source, target, label, routing, true)
}

fn connect_inner(
    editor: &mut Editor,
    source: Endpoint,
    target: Endpoint,
    label: &str,
    routing: Routing,
    own_transaction: bool,
) -> Result<NodeId, String> {
    let diagram = editor
        .doc
        .diagram
        .as_deref()
        .ok_or("Add diagram shapes first")?;
    if !diagram.contains_endpoint(source.shape)
        || !diagram.contains_endpoint(target.shape)
        || !source.port.valid()
        || !target.port.valid()
        || label.chars().count() > crate::text::MAX_CHARS
    {
        return Err("Choose existing shapes and valid ports.".into());
    }
    if diagram.edges.len() >= MAX_EDGES {
        return Err("Connector limit reached.".into());
    }
    if own_transaction && editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    // Match Builder::finish: opaque container subtrees must paint before
    // root connectors, including connections between nested shapes.
    let index = if diagram
        .shapes
        .values()
        .any(|shape| shape.kind.is_container())
    {
        usize::MAX
    } else {
        editor
            .doc
            .children(None)
            .iter()
            .position(|id| diagram.shapes.contains_key(id))
            .unwrap_or(usize::MAX)
    };
    if own_transaction {
        editor.begin("Connect diagram shapes");
    }
    let result = (|| {
        let group = editor
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Connector",
                    NodeKind::Group { collapsed: false },
                )),
                slot: Slot {
                    parent: None,
                    index,
                },
            })
            .map_err(|e| e.to_string())?
            .unwrap();
        let mut paths = Vec::new();
        for (name, fill) in [("Connection", None), ("Arrow", Some(DEFAULT_LINE))] {
            paths.push(
                editor
                    .execute(Command::AddNode {
                        node: Box::new(Node::path(
                            0,
                            name,
                            Arc::new(Path::default()),
                            PathStyle {
                                fill,
                                stroke: Some(DEFAULT_LINE),
                                width: DEFAULT_LINE_WIDTH,
                                ..Default::default()
                            },
                            editor.doc.width,
                            editor.doc.height,
                        )),
                        slot: Slot::top_of(Some(group)),
                    })
                    .map_err(|e| e.to_string())?
                    .unwrap(),
            );
        }
        let label = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Connector label",
                    TextSpec {
                        text: label.into(),
                        font: "Geist".into(),
                        size: 12.,
                        width: Some(120.),
                        align: Align::Center,
                        color: DEFAULT_TEXT,
                        ..Default::default()
                    },
                    editor.doc.width,
                    editor.doc.height,
                )),
                slot: Slot::top_of(Some(group)),
            })
            .map_err(|e| e.to_string())?
            .unwrap();
        let mut diagram = editor.doc.diagram.as_deref().cloned().unwrap_or_default();
        diagram.edges.insert(
            group,
            Edge {
                routing_warning: None,
                double_line: false,
                label_background: None,
                double_path: None,
                label_background_path: None,
                corner_radius: 0.,
                labels: Vec::new(),
                jump_style: JumpStyle::None,
                jump_size: 10.,
                path: paths[0],
                arrow: paths[1],
                label,
                source,
                target,
                routing,
                waypoints: Vec::new(),
                label_offset: (0., 0.),
                label_position: 0.,
                label_normal: 0.,
                start_marker: Marker::default(),
                end_marker: Marker::default(),
                arrow_end: true,
                arrow_start: false,
            },
        );
        editor
            .execute(Command::SetDiagram {
                diagram: Some(Arc::new(diagram)),
            })
            .map_err(|e| e.to_string())?;
        workspace::apply_default(editor, group, true)?;
        Ok(group)
    })();
    match result {
        Ok(id) => {
            if own_transaction {
                editor.end();
            }
            Ok(id)
        }
        Err(error) => {
            if own_transaction {
                editor.cancel();
            }
            Err(error)
        }
    }
}

/// Add a bound neighboring shape as one undo step. Occupied space is skipped;
/// the original shape and any existing connections are never moved.
pub fn quick_create(
    editor: &mut Editor,
    source: NodeId,
    direction: Port,
    kind: ShapeKind,
) -> Result<NodeId, String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let model = editor
        .doc
        .diagram
        .as_ref()
        .ok_or("Select a diagram shape first.")?;
    let shape = model
        .shapes
        .get(&source)
        .ok_or("Select a diagram shape first.")?;
    let [x, y, w, h] = shape_bounds(&editor.doc, shape).ok_or("Missing shape bounds.")?;
    let container = shape.container;
    let (dx, dy, opposite) = match direction {
        Port::East => (1., 0., Port::West),
        Port::West => (-1., 0., Port::East),
        Port::North => (0., -1., Port::South),
        Port::South => (0., 1., Port::North),
        _ => return Err("Choose a cardinal direction.".into()),
    };
    let occupied = model
        .shapes
        .values()
        .filter(|s| !s.kind.is_container())
        .filter_map(|s| shape_bounds(&editor.doc, s))
        .collect::<Vec<_>>();
    let bounds = (1..=MAX_SHAPES + 1)
        .map(|i| {
            [
                x + dx * (w + 60.) * i as f64,
                y + dy * (h + 60.) * i as f64,
                w,
                h,
            ]
        })
        .find(|b| {
            valid_bounds(*b)
                && !occupied.iter().any(|r| {
                    b[0] < r[0] + r[2] + 20.
                        && b[0] + b[2] + 20. > r[0]
                        && b[1] < r[1] + r[3] + 20.
                        && b[1] + b[3] + 20. > r[1]
                })
        })
        .ok_or("No room in that direction. Move the source or choose another direction.")?;
    editor.begin("Add connected shape");
    let result = (|| {
        let id = add_shape_inner(editor, kind, bounds, kind.label(), false)?;
        if container.is_some() {
            editor
                .execute(Command::MoveNode {
                    id,
                    slot: Slot::top_of(container),
                })
                .map_err(|e| e.to_string())?;
        }
        connect_inner(
            editor,
            Endpoint {
                shape: source,
                port: direction,
            },
            Endpoint {
                shape: id,
                port: opposite,
            },
            "",
            Routing::Orthogonal,
            false,
        )?;
        Ok(id)
    })();
    if result.is_ok() {
        editor.end();
    } else {
        editor.cancel();
    }
    result
}

#[cfg(test)]
#[path = "diagram_tests.rs"]
mod tests;

#[path = "diagram_object.rs"]
mod object;
pub use object::{ObjectStyle, connector_style_command, object_details_commands};

#[path = "diagram_structure.rs"]
pub mod structure;
#[path = "diagram_workspace.rs"]
pub mod workspace;

#[path = "diagram_catalog.rs"]
mod catalog;
pub use catalog::{DocumentStencil, document_stencil, document_stencils, insert_document_stencil};

/// Decorative corner/jump anchors are not semantic bends; transform their stored bends directly.
pub(crate) fn transform_decorated_waypoints(
    doc: &mut Document,
    ids: &HashSet<NodeId>,
    transform: glam::DAffine2,
) {
    let Some(model) = doc.diagram.as_ref() else {
        return;
    };
    if !model.edges.values().any(|e| {
        ids.contains(&e.path)
            && !e.waypoints.is_empty()
            && (e.corner_radius > 0. || e.jump_style != JumpStyle::None)
    }) {
        return;
    }
    let model = Arc::make_mut(doc.diagram.as_mut().unwrap());
    for edge in model.edges.values_mut().filter(|e| {
        ids.contains(&e.path) && (e.corner_radius > 0. || e.jump_style != JumpStyle::None)
    }) {
        for p in &mut edge.waypoints {
            let next = transform.transform_point2(glam::dvec2(p.0, p.1));
            *p = (next.x, next.y);
        }
    }
}

/// Preserve the actual picked location under affine transforms, not its AABB fraction.
pub(crate) fn transformed_attachments(
    doc: &Document,
    ids: &HashSet<NodeId>,
    transform: glam::DAffine2,
) -> Vec<(NodeId, bool, NodeId, glam::DVec2)> {
    let Some(model) = &doc.diagram else {
        return Vec::new();
    };
    model
        .edges
        .iter()
        .flat_map(|(id, edge)| {
            [(false, &edge.source), (true, &edge.target)]
                .into_iter()
                .filter_map(move |(target, endpoint)| {
                    if !matches!(endpoint.port, Port::Custom { .. }) {
                        return None;
                    }
                    let shape = model.shapes.get(&endpoint.shape)?;
                    if !ids.contains(&shape.body) {
                        return None;
                    }
                    let p = endpoint.port.anchor(shape_bounds(doc, shape)?, (0., 0.)).0;
                    Some((
                        *id,
                        target,
                        endpoint.shape,
                        transform.transform_point2(glam::dvec2(p.0, p.1)),
                    ))
                })
        })
        .collect()
}
pub(crate) fn apply_transformed_attachments(
    doc: &mut Document,
    points: Vec<(NodeId, bool, NodeId, glam::DVec2)>,
) {
    if points.is_empty() {
        return;
    }
    let updates = points
        .into_iter()
        .filter_map(|(id, target, shape, p)| {
            let bounds = shape_bounds(doc, doc.diagram.as_ref()?.shapes.get(&shape)?)?;
            Some((
                id,
                target,
                Port::Custom {
                    x: (p.x - bounds[0]) / bounds[2],
                    y: (p.y - bounds[1]) / bounds[3],
                },
            ))
        })
        .collect::<Vec<_>>();
    if let Some(model) = doc.diagram.as_mut() {
        let model = Arc::make_mut(model);
        for (id, target, port) in updates {
            if let Some(edge) = model.edges.get_mut(&id) {
                if target {
                    edge.target.port = port;
                } else {
                    edge.source.port = port;
                }
            }
        }
    }
}
