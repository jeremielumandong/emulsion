//! The Command API: the only way to change a document. The UI, the MCP server
//! and scripting all go through here, so the assistant has exactly the powers
//! a person has.

use crate::document::Document;
use crate::node::{Node, NodeId, NodeKind};
use emulsion_raster::vector::{Path, PathStyle};
use emulsion_raster::{Adjustment, BlendMode, IRect, Mask, Placement, Raster};
use std::sync::Arc;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CommandError {
    #[error("no node with id {0}")]
    NoSuchNode(NodeId),
    #[error("node {0} is not a group")]
    NotAGroup(NodeId),
    #[error("cannot move a group into itself")]
    IntoItself,
    #[error("node {0} has no parameter {1}")]
    NoSuchParam(NodeId, String),
    #[error("nothing to group")]
    Empty,
    #[error("the result is invalid: {0}")]
    Invalid(#[from] crate::document::DocumentError),
}

/// Where a node goes: a parent (None = top level) and an index among that
/// parent's children, counted from the bottom. `usize::MAX` means on top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub parent: Option<NodeId>,
    pub index: usize,
}

impl Slot {
    pub const TOP: Slot = Slot {
        parent: None,
        index: usize::MAX,
    };
    pub fn top_of(parent: Option<NodeId>) -> Self {
        Slot {
            parent,
            index: usize::MAX,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    /// Insert a node. Its `id` is ignored and a fresh one allocated.
    AddNode {
        node: Box<Node>,
        slot: Slot,
    },
    RemoveNode {
        id: NodeId,
    },
    MoveNode {
        id: NodeId,
        slot: Slot,
    },
    DuplicateNode {
        id: NodeId,
    },
    SetVisible {
        id: NodeId,
        visible: bool,
    },
    SetLocked {
        id: NodeId,
        locked: bool,
    },
    SetOpacity {
        id: NodeId,
        opacity: f32,
    },
    SetBlend {
        id: NodeId,
        blend: BlendMode,
    },
    Rename {
        id: NodeId,
        name: String,
    },
    SetParam {
        id: NodeId,
        key: String,
        value: f32,
    },
    SetAdjustment {
        id: NodeId,
        adjustment: Adjustment,
    },
    SetPlacement {
        id: NodeId,
        placement: Placement,
    },
    SetClip {
        id: NodeId,
        clip_to: Option<NodeId>,
    },
    SetMaskEnabled {
        id: NodeId,
        enabled: bool,
    },
    SetCollapsed {
        id: NodeId,
        collapsed: bool,
    },
    /// Wrap `ids` (siblings or not) in a new group placed where the topmost
    /// of them was.
    Group {
        ids: Vec<NodeId>,
        name: String,
    },
    /// Replace a group by its children.
    Ungroup {
        id: NodeId,
    },
    /// Set or clear the selection (document-space coverage).
    SetSelection {
        selection: Option<Arc<Mask>>,
    },
    /// Replace a pixel node's pixels. `dirty` is the changed area in the
    /// node's own pixel space, for re-rendering only what changed.
    ReplacePixels {
        id: NodeId,
        raster: Arc<Raster>,
        dirty: IRect,
        label: String,
    },
    /// Set or clear a node's mask.
    SetMask {
        id: NodeId,
        mask: Option<Arc<Mask>>,
    },
    /// Rotate everything by `rotation` degrees clockwise about the canvas
    /// centre, then crop to `rect` (which may extend past the canvas).
    /// Pixel nodes are moved, never resampled.
    Crop {
        rect: IRect,
        rotation: f64,
    },
    /// Scale the canvas and every placement. Pixel nodes keep their source
    /// pixels, so this is lossless and reversible.
    ImageSize {
        width: u32,
        height: u32,
    },
    /// Replace a pixel node's pixels, mask and placement together, for
    /// operations that change its size (Distort). The mask, if any, must
    /// match the new pixels.
    ReplaceContent {
        id: NodeId,
        raster: Arc<Raster>,
        mask: Option<Arc<Mask>>,
        placement: Placement,
        label: String,
    },
    /// Replace a Path node's path and style; it is rasterized again.
    SetPath {
        id: NodeId,
        path: Arc<Path>,
        style: PathStyle,
    },
    /// Replace a text layer's content and style.
    SetText {
        id: NodeId,
        spec: Box<crate::text::TextSpec>,
    },
    /// Turn a pixel node into a smart layer with an empty filter stack.
    ConvertToSmart {
        id: NodeId,
    },
    /// Bake a smart layer's filters into pixels.
    Rasterize {
        id: NodeId,
    },
    /// Replace a smart layer's filter stack; the cache is rendered again.
    SetFilters {
        id: NodeId,
        filters: Vec<emulsion_filters::Filter>,
    },
    /// `SetFilters` with the stack already rendered (off the UI thread).
    SetSmartCache {
        id: NodeId,
        filters: Vec<emulsion_filters::Filter>,
        cache: Arc<emulsion_raster::Raster>,
        offset: (i32, i32),
    },
    /// Replace a node's layer styles.
    SetStyles {
        id: NodeId,
        styles: Vec<crate::styles::LayerStyle>,
    },
    /// Replace the ruler guides.
    SetGuides {
        guides: Vec<crate::document::Guide>,
    },
}

/// What a command changed, for re-rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dirty {
    /// Nothing visible.
    Nothing,
    /// Only this document-space rectangle.
    Rect(IRect),
    /// Everything.
    All,
}

impl Dirty {
    pub fn union(self, o: Dirty) -> Dirty {
        match (self, o) {
            (Dirty::Nothing, x) | (x, Dirty::Nothing) => x,
            (Dirty::Rect(a), Dirty::Rect(b)) => Dirty::Rect(a.union(&b)),
            _ => Dirty::All,
        }
    }
}

impl Command {
    /// Human-readable name for the history list.
    pub fn label(&self) -> String {
        match self {
            Command::AddNode { node, .. } => format!("Add {}", node.name),
            Command::RemoveNode { .. } => "Delete node".into(),
            Command::MoveNode { .. } => "Reorder".into(),
            Command::DuplicateNode { .. } => "Duplicate".into(),
            Command::SetVisible { visible, .. } => if *visible { "Show" } else { "Hide" }.into(),
            Command::SetLocked { locked, .. } => if *locked { "Lock" } else { "Unlock" }.into(),
            Command::SetOpacity { .. } => "Opacity".into(),
            Command::SetBlend { blend, .. } => format!("Blend: {}", blend.label()),
            Command::Rename { name, .. } => format!("Rename to {name}"),
            Command::SetParam { key, .. } => key.replace('_', " "),
            Command::SetAdjustment { .. } => "Adjustment".into(),
            Command::SetPlacement { .. } => "Transform".into(),
            Command::SetClip { clip_to, .. } => {
                if clip_to.is_some() { "Clip" } else { "Unclip" }.into()
            }
            Command::SetMaskEnabled { .. } => "Toggle mask".into(),
            Command::SetCollapsed { .. } => "Collapse".into(),
            Command::Group { .. } => "Group".into(),
            Command::Ungroup { .. } => "Ungroup".into(),
            Command::SetSelection { selection } => if selection.is_some() {
                "Select"
            } else {
                "Deselect"
            }
            .into(),
            Command::ReplacePixels { label, .. } => label.clone(),
            Command::SetMask { mask, .. } => if mask.is_some() {
                "Mask"
            } else {
                "Remove mask"
            }
            .into(),
            Command::Crop { rotation, .. } => if *rotation != 0.0 {
                "Straighten and crop"
            } else {
                "Crop"
            }
            .into(),
            Command::ImageSize { .. } => "Image size".into(),
            Command::SetGuides { .. } => "Guides".into(),
            Command::ReplaceContent { label, .. } => label.clone(),
            Command::SetPath { .. } => "Edit path".into(),
            Command::SetText { .. } => "Edit text".into(),
            Command::SetStyles { styles, .. } => match styles.last() {
                Some(s) => s.label().to_string(),
                None => "Layer styles".into(),
            },
            Command::ConvertToSmart { .. } => "Smart layer".into(),
            Command::Rasterize { .. } => "Rasterize".into(),
            Command::SetFilters { filters, .. } | Command::SetSmartCache { filters, .. } => {
                match filters.last() {
                    Some(f) => f.label().to_string(),
                    None => "Filters".into(),
                }
            }
        }
    }

    /// The region this command changes on screen, given the document before.
    pub fn dirty(&self, before: &Document) -> Dirty {
        match self {
            Command::SetSelection { .. }
            | Command::SetGuides { .. }
            | Command::SetCollapsed { .. }
            | Command::SetLocked { .. }
            | Command::Rename { .. } => Dirty::Nothing,
            Command::SetPath { id, path, style } => match before.node(*id).map(|n| &n.kind) {
                Some(NodeKind::Path { cache, .. }) => Dirty::Rect(
                    cache
                        .tile_bounds()
                        .union(&path.bounds(style))
                        .intersect(&IRect::new(0, 0, before.width as i32, before.height as i32)),
                ),
                _ => Dirty::All,
            },
            Command::SetText { id, .. } => match before.node(*id).map(|n| &n.kind) {
                // The new extent is only known after shaping; old + whole is safe.
                Some(NodeKind::Text { .. }) => Dirty::All,
                _ => Dirty::All,
            },
            Command::ReplacePixels { id, dirty, .. } => match before.node(*id).map(|n| &n.kind) {
                Some(NodeKind::Raster { placement, .. }) if !dirty.is_empty() => {
                    let m = placement.to_doc(1, 1);
                    let corners = [
                        (dirty.x, dirty.y),
                        (dirty.right(), dirty.y),
                        (dirty.x, dirty.bottom()),
                        (dirty.right(), dirty.bottom()),
                    ]
                    .map(|(x, y)| m.transform_point2(glam::dvec2(x as f64, y as f64)));
                    let (mut lo, mut hi) = (corners[0], corners[0]);
                    for c in &corners[1..] {
                        lo = lo.min(*c);
                        hi = hi.max(*c);
                    }
                    let (x0, y0) = (lo.x.floor() as i32 - 1, lo.y.floor() as i32 - 1);
                    Dirty::Rect(IRect::new(
                        x0,
                        y0,
                        hi.x.ceil() as i32 + 1 - x0,
                        hi.y.ceil() as i32 + 1 - y0,
                    ))
                }
                _ => Dirty::All,
            },
            _ => Dirty::All,
        }
    }

    /// Whether the command changes what is rendered or saved, as opposed to
    /// panel state such as collapsing a group.
    pub fn is_view_only(&self) -> bool {
        matches!(self, Command::SetCollapsed { .. })
    }

    /// Apply to `doc`. Returns the id of a created node, if any. On error the
    /// document is unchanged.
    pub fn apply(&self, doc: &mut Document) -> Result<Option<NodeId>, CommandError> {
        let mut next = doc.clone();
        let created = self.apply_inner(&mut next)?;
        next.normalize();
        next.validate()?;
        *doc = next;
        Ok(created)
    }

    fn apply_inner(&self, doc: &mut Document) -> Result<Option<NodeId>, CommandError> {
        let need = |doc: &Document, id: NodeId| {
            doc.node(id).map(|_| ()).ok_or(CommandError::NoSuchNode(id))
        };
        match self {
            Command::AddNode { node, slot } => {
                if let Some(p) = slot.parent {
                    need(doc, p)?;
                    if !doc.node(p).unwrap().is_group() {
                        return Err(CommandError::NotAGroup(p));
                    }
                }
                let mut n = (**node).clone();
                n.id = doc.alloc_id();
                n.parent = slot.parent;
                n.clip_to = None;
                let id = n.id;
                insert_at(doc, n, *slot);
                Ok(Some(id))
            }
            Command::RemoveNode { id } => {
                need(doc, *id)?;
                let gone = doc.subtree(*id);
                doc.nodes.retain(|n| !gone.contains(&n.id));
                for n in &mut doc.nodes {
                    if n.clip_to.is_some_and(|c| gone.contains(&c)) {
                        n.clip_to = None;
                    }
                }
                Ok(None)
            }
            Command::MoveNode { id, slot } => {
                need(doc, *id)?;
                if let Some(p) = slot.parent {
                    need(doc, p)?;
                    if !doc.node(p).unwrap().is_group() {
                        return Err(CommandError::NotAGroup(p));
                    }
                    if p == *id || doc.is_ancestor(*id, p) {
                        return Err(CommandError::IntoItself);
                    }
                }
                let moving = doc.subtree(*id);
                let block: Vec<Node> = doc
                    .nodes
                    .iter()
                    .filter(|n| moving.contains(&n.id))
                    .cloned()
                    .collect();
                doc.nodes.retain(|n| !moving.contains(&n.id));
                let old_parent = block.iter().find(|n| n.id == *id).unwrap().parent;
                // Clipping only holds between siblings; moving breaks it.
                if old_parent != slot.parent {
                    for n in &mut doc.nodes {
                        if n.clip_to == Some(*id) {
                            n.clip_to = None;
                        }
                    }
                }
                let mut root = block.iter().find(|n| n.id == *id).unwrap().clone();
                root.parent = slot.parent;
                root.clip_to = None;
                insert_at(doc, root, *slot);
                // Descendants go back in; normalize() restores order.
                doc.nodes.extend(block.into_iter().filter(|n| n.id != *id));
                fix_clips(doc);
                Ok(None)
            }
            Command::DuplicateNode { id } => {
                need(doc, *id)?;
                let ids = doc.subtree(*id);
                let mut map = std::collections::HashMap::new();
                for old in &ids {
                    map.insert(*old, doc.alloc_id());
                }
                let src = doc.node(*id).unwrap().clone();
                let pos = doc
                    .children(src.parent)
                    .iter()
                    .position(|s| s == id)
                    .unwrap()
                    + 1;
                let mut copies: Vec<Node> = doc
                    .nodes
                    .iter()
                    .filter(|n| ids.contains(&n.id))
                    .cloned()
                    .collect();
                for n in &mut copies {
                    n.id = map[&n.id];
                    if n.id == map[id] {
                        n.name = format!("{} copy", n.name);
                        n.clip_to = None;
                    } else {
                        n.parent = n.parent.map(|p| map[&p]);
                        n.clip_to = n.clip_to.map(|c| map.get(&c).copied().unwrap_or(c));
                    }
                }
                let root = copies.iter().position(|n| n.id == map[id]).unwrap();
                let root = copies.remove(root);
                insert_at(
                    doc,
                    root,
                    Slot {
                        parent: src.parent,
                        index: pos,
                    },
                );
                doc.nodes.extend(copies);
                Ok(Some(map[id]))
            }
            Command::SetVisible { id, visible } => set(doc, *id, |n| n.visible = *visible),
            Command::SetLocked { id, locked } => set(doc, *id, |n| n.locked = *locked),
            Command::SetOpacity { id, opacity } => {
                set(doc, *id, |n| n.opacity = opacity.clamp(0.0, 1.0))
            }
            Command::SetBlend { id, blend } => {
                let is_group = doc
                    .node(*id)
                    .ok_or(CommandError::NoSuchNode(*id))?
                    .is_group();
                let b = if *blend == BlendMode::PassThrough && !is_group {
                    BlendMode::Normal
                } else {
                    *blend
                };
                set(doc, *id, |n| n.blend = b)
            }
            Command::Rename { id, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Ok(None);
                }
                set(doc, *id, |n| n.name = name.chars().take(256).collect())
            }
            Command::SetParam { id, key, value } => {
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                let ok = match &mut n.kind {
                    NodeKind::Adjust(a) => a.set_param(key, *value),
                    _ => false,
                };
                if ok {
                    Ok(None)
                } else {
                    Err(CommandError::NoSuchParam(*id, key.clone()))
                }
            }
            Command::SetAdjustment { id, adjustment } => {
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                match &mut n.kind {
                    NodeKind::Adjust(a) => {
                        *a = adjustment.clone();
                        Ok(None)
                    }
                    _ => Err(CommandError::NoSuchParam(*id, "adjustment".into())),
                }
            }
            Command::SetPlacement { id, placement } => {
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                match &mut n.kind {
                    NodeKind::Raster { placement: p, .. }
                    | NodeKind::Smart { placement: p, .. } => {
                        *p = *placement;
                        Ok(None)
                    }
                    _ => Err(CommandError::NoSuchParam(*id, "placement".into())),
                }
            }
            Command::SetClip { id, clip_to } => {
                need(doc, *id)?;
                let target = *clip_to;
                set(doc, *id, |n| n.clip_to = target)
            }
            Command::SetMaskEnabled { id, enabled } => set(doc, *id, |n| n.mask_enabled = *enabled),
            Command::SetCollapsed { id, collapsed } => {
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                match &mut n.kind {
                    NodeKind::Group { collapsed: c } => {
                        *c = *collapsed;
                        Ok(None)
                    }
                    _ => Err(CommandError::NotAGroup(*id)),
                }
            }
            Command::Group { ids, name } => {
                let mut ids: Vec<NodeId> = ids
                    .iter()
                    .copied()
                    .filter(|i| doc.node(*i).is_some())
                    .collect();
                // Drop ids already covered by a selected ancestor.
                let all = ids.clone();
                ids.retain(|i| !all.iter().any(|a| a != i && doc.is_ancestor(*a, *i)));
                if ids.is_empty() {
                    return Err(CommandError::Empty);
                }
                // Stack order, bottom to top.
                ids.sort_by_key(|i| doc.index_of(*i).unwrap());
                let top = *ids.last().unwrap();
                let parent = doc.node(top).unwrap().parent;
                let pos = doc.children(parent).iter().position(|s| *s == top).unwrap();
                let gid = doc.alloc_id();
                let mut g = Node::group(gid, name.clone());
                g.parent = parent;
                insert_at(
                    doc,
                    g,
                    Slot {
                        parent,
                        index: pos + 1,
                    },
                );
                for id in &ids {
                    let n = doc.node_mut(*id).unwrap();
                    n.parent = Some(gid);
                }
                // Keep relative stack order inside the group.
                let order: Vec<NodeId> = ids.clone();
                let mut members: Vec<Node> = Vec::new();
                doc.nodes.retain(|n| {
                    if order.contains(&n.id) {
                        members.push(n.clone());
                        false
                    } else {
                        true
                    }
                });
                members.sort_by_key(|n| order.iter().position(|o| *o == n.id));
                let gpos = doc.index_of(gid).unwrap();
                for (k, m) in members.into_iter().enumerate() {
                    doc.nodes.insert(gpos + k, m);
                }
                fix_clips(doc);
                Ok(Some(gid))
            }
            Command::SetSelection { selection } => {
                if let Some(m) = selection
                    && (m.width() != doc.width || m.height() != doc.height)
                {
                    return Err(CommandError::Invalid(
                        crate::document::DocumentError::BadValue(0, "selection size"),
                    ));
                }
                doc.selection = selection.clone();
                Ok(None)
            }
            Command::ReplacePixels { id, raster, .. } => {
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                match &mut n.kind {
                    NodeKind::Raster { raster: r, .. } => {
                        *r = raster.clone();
                        Ok(None)
                    }
                    _ => Err(CommandError::NoSuchParam(*id, "pixels".into())),
                }
            }
            Command::SetMask { id, mask } => {
                let n = doc.node(*id).ok_or(CommandError::NoSuchNode(*id))?;
                let (w, h) = match &n.kind {
                    NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
                    _ => (doc.width, doc.height),
                };
                if let Some(m) = mask
                    && (m.width() != w || m.height() != h)
                {
                    return Err(CommandError::Invalid(
                        crate::document::DocumentError::BadValue(*id, "mask size"),
                    ));
                }
                set(doc, *id, |n| n.mask = mask.clone())
            }
            Command::Crop { rect, rotation } => {
                crate::geometry::crop(doc, *rect, *rotation);
                Ok(None)
            }
            Command::ImageSize { width, height } => {
                crate::geometry::resize(doc, *width, *height);
                Ok(None)
            }
            Command::SetGuides { guides } => {
                doc.guides = guides.clone();
                Ok(None)
            }
            Command::SetStyles { id, styles } => {
                if styles.len() > crate::styles::MAX_STYLES {
                    return Err(CommandError::Invalid(
                        crate::document::DocumentError::BadValue(*id, "too many styles"),
                    ));
                }
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                if !matches!(
                    n.kind,
                    NodeKind::Raster { .. }
                        | NodeKind::Smart { .. }
                        | NodeKind::Path { .. }
                        | NodeKind::Text { .. }
                ) {
                    return Err(CommandError::NoSuchParam(*id, "styles".into()));
                }
                n.styles = styles.clone();
                Ok(None)
            }
            Command::ConvertToSmart { id } => {
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                let NodeKind::Raster { raster, placement } = &n.kind else {
                    return Err(CommandError::NoSuchParam(*id, "pixels".into()));
                };
                n.kind = NodeKind::Smart {
                    source: raster.clone(),
                    filters: Vec::new(),
                    placement: *placement,
                    cache: raster.clone(),
                    offset: (0, 0),
                };
                Ok(None)
            }
            Command::Rasterize { id } => {
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                let NodeKind::Smart {
                    source,
                    placement,
                    cache,
                    offset,
                    ..
                } = &n.kind
                else {
                    return Err(CommandError::NoSuchParam(*id, "filters".into()));
                };
                let p = crate::smart::cache_placement(
                    placement,
                    (source.width(), source.height()),
                    (cache.width(), cache.height()),
                    *offset,
                );
                n.kind = NodeKind::Raster {
                    raster: cache.clone(),
                    placement: p,
                };
                Ok(None)
            }
            Command::SetFilters { id, filters } => {
                if filters.len() > 32 {
                    return Err(CommandError::Invalid(
                        crate::document::DocumentError::BadValue(*id, "too many filters"),
                    ));
                }
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                let NodeKind::Smart {
                    source,
                    filters: f,
                    cache,
                    offset,
                    ..
                } = &mut n.kind
                else {
                    return Err(CommandError::NoSuchParam(*id, "filters".into()));
                };
                let (c, o) = crate::smart::render(source, filters);
                *cache = c;
                *offset = o;
                *f = filters.clone();
                Ok(None)
            }
            Command::SetSmartCache {
                id,
                filters,
                cache: rendered,
                offset: off,
            } => {
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                let NodeKind::Smart {
                    filters: f,
                    cache,
                    offset,
                    ..
                } = &mut n.kind
                else {
                    return Err(CommandError::NoSuchParam(*id, "filters".into()));
                };
                *cache = rendered.clone();
                *offset = *off;
                *f = filters.clone();
                Ok(None)
            }
            Command::SetPath { id, path, style } => {
                if path.anchor_count() > emulsion_raster::vector::MAX_ANCHORS {
                    return Err(CommandError::Invalid(
                        crate::document::DocumentError::BadValue(*id, "too many anchors"),
                    ));
                }
                let (w, h) = (doc.width, doc.height);
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                match &mut n.kind {
                    NodeKind::Path {
                        path: p,
                        style: s,
                        cache,
                    } => {
                        let style = style.sanitized();
                        *cache = Arc::new(path.rasterize(&style, w, h));
                        *p = path.clone();
                        *s = style;
                        Ok(None)
                    }
                    _ => Err(CommandError::NoSuchParam(*id, "path".into())),
                }
            }
            Command::SetText { id, spec } => {
                let (w, h) = (doc.width, doc.height);
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                match &mut n.kind {
                    NodeKind::Text { spec: s, cache } => {
                        let spec = (**spec).clone().sanitized();
                        *cache = Arc::new(crate::text::rasterize(&spec, w, h));
                        *s = Arc::new(spec);
                        Ok(None)
                    }
                    _ => Err(CommandError::NoSuchParam(*id, "text".into())),
                }
            }
            Command::ReplaceContent {
                id,
                raster,
                mask,
                placement,
                ..
            } => {
                if let Some(m) = mask
                    && (m.width() != raster.width() || m.height() != raster.height())
                {
                    return Err(CommandError::Invalid(
                        crate::document::DocumentError::BadValue(*id, "mask size"),
                    ));
                }
                let n = doc.node_mut(*id).ok_or(CommandError::NoSuchNode(*id))?;
                match &mut n.kind {
                    NodeKind::Raster {
                        raster: r,
                        placement: p,
                    } => {
                        *r = raster.clone();
                        *p = *placement;
                        n.mask = mask.clone();
                        Ok(None)
                    }
                    _ => Err(CommandError::NoSuchParam(*id, "pixels".into())),
                }
            }
            Command::Ungroup { id } => {
                let g = doc.node(*id).ok_or(CommandError::NoSuchNode(*id))?.clone();
                if !g.is_group() {
                    return Err(CommandError::NotAGroup(*id));
                }
                for n in &mut doc.nodes {
                    if n.parent == Some(*id) {
                        n.parent = g.parent;
                    }
                    if n.clip_to == Some(*id) {
                        n.clip_to = None;
                    }
                }
                doc.nodes.retain(|n| n.id != *id);
                fix_clips(doc);
                Ok(None)
            }
        }
    }
}

fn set(
    doc: &mut Document,
    id: NodeId,
    f: impl FnOnce(&mut Node),
) -> Result<Option<NodeId>, CommandError> {
    let n = doc.node_mut(id).ok_or(CommandError::NoSuchNode(id))?;
    f(n);
    Ok(None)
}

/// Insert `n` among `slot.parent`'s children at `slot.index` (from the
/// bottom). The flat position is fixed up by `normalize()`.
fn insert_at(doc: &mut Document, n: Node, slot: Slot) {
    let siblings = doc.children(slot.parent);
    let flat = if siblings.is_empty() {
        match slot.parent {
            Some(p) => doc.index_of(p).unwrap_or(doc.nodes.len()),
            None => doc.nodes.len(),
        }
    } else if slot.index >= siblings.len() {
        doc.index_of(*siblings.last().unwrap()).unwrap() + 1
    } else {
        // Before the block of the sibling currently at that index.
        let s = siblings[slot.index];
        let size = doc.subtree(s).len() - 1;
        doc.index_of(s).unwrap() - size
    };
    doc.nodes.insert(flat.min(doc.nodes.len()), n);
}

/// Drop clip references that no longer point at a sibling below.
fn fix_clips(doc: &mut Document) {
    let snapshot = doc.clone();
    for n in &mut doc.nodes {
        if let Some(c) = n.clip_to {
            let sib = snapshot.children(n.parent);
            let me = sib.iter().position(|s| *s == n.id);
            let base = sib.iter().position(|s| *s == c);
            if !matches!((me, base), (Some(m), Some(b)) if b < m) {
                n.clip_to = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_raster::Raster;
    use std::sync::Arc;

    fn doc3() -> (Document, [NodeId; 3]) {
        let mut d = Document::new(64, 64);
        let mut ids = [0; 3];
        for (i, name) in ["bottom", "middle", "top"].iter().enumerate() {
            let n = Node::raster(
                0,
                *name,
                Arc::new(Raster::transparent(64, 64)),
                Placement::default(),
            );
            ids[i] = Command::AddNode {
                node: Box::new(n),
                slot: Slot::TOP,
            }
            .apply(&mut d)
            .unwrap()
            .unwrap();
        }
        (d, ids)
    }

    fn names(d: &Document) -> Vec<String> {
        d.nodes.iter().map(|n| n.name.clone()).collect()
    }

    #[test]
    fn add_and_move() {
        let (mut d, [b, _, t]) = doc3();
        assert_eq!(names(&d), ["bottom", "middle", "top"]);
        Command::MoveNode {
            id: t,
            slot: Slot {
                parent: None,
                index: 0,
            },
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!(names(&d), ["top", "bottom", "middle"]);
        Command::MoveNode {
            id: b,
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!(names(&d), ["top", "middle", "bottom"]);
    }

    #[test]
    fn group_keeps_order_and_contiguity() {
        let (mut d, [b, m, t]) = doc3();
        let g = Command::Group {
            ids: vec![t, b],
            name: "g".into(),
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        assert_eq!(names(&d), ["middle", "bottom", "top", "g"]);
        assert_eq!(d.children(Some(g)), vec![b, t]);
        d.validate().unwrap();
        // Moving the group moves its block.
        Command::MoveNode {
            id: g,
            slot: Slot {
                parent: None,
                index: 0,
            },
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!(names(&d), ["bottom", "top", "g", "middle"]);
        Command::Ungroup { id: g }.apply(&mut d).unwrap();
        assert_eq!(names(&d), ["bottom", "top", "middle"]);
        assert!(d.node(m).unwrap().parent.is_none());
    }

    #[test]
    fn cannot_move_group_into_itself() {
        let (mut d, [b, _, t]) = doc3();
        let g = Command::Group {
            ids: vec![b, t],
            name: "g".into(),
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        let inner = Command::Group {
            ids: vec![t],
            name: "inner".into(),
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        let err = Command::MoveNode {
            id: g,
            slot: Slot::top_of(Some(inner)),
        }
        .apply(&mut d)
        .unwrap_err();
        assert_eq!(err, CommandError::IntoItself);
    }

    #[test]
    fn clip_is_validated_and_dropped_on_move() {
        let (mut d, [b, m, _]) = doc3();
        Command::SetClip {
            id: m,
            clip_to: Some(b),
        }
        .apply(&mut d)
        .unwrap();
        assert!(
            Command::SetClip {
                id: b,
                clip_to: Some(m)
            }
            .apply(&mut d)
            .is_err(),
            "base must be below"
        );
        Command::MoveNode {
            id: b,
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!(d.node(m).unwrap().clip_to, None);
    }

    #[test]
    fn remove_and_duplicate_subtree() {
        let (mut d, [b, m, t]) = doc3();
        let g = Command::Group {
            ids: vec![b, m],
            name: "g".into(),
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        let copy = Command::DuplicateNode { id: g }
            .apply(&mut d)
            .unwrap()
            .unwrap();
        assert_eq!(d.nodes.len(), 7);
        assert_eq!(d.children(Some(copy)).len(), 2);
        d.validate().unwrap();
        Command::RemoveNode { id: g }.apply(&mut d).unwrap();
        assert_eq!(d.nodes.len(), 4);
        assert!(d.node(t).is_some());
    }

    #[test]
    fn param_edits_and_errors_leave_doc_unchanged() {
        let (mut d, _) = doc3();
        let a = Command::AddNode {
            node: Box::new(Node::adjust(
                0,
                Adjustment::Exposure {
                    exposure: 0.0,
                    offset: 0.0,
                    gamma: 1.0,
                },
            )),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        Command::SetParam {
            id: a,
            key: "exposure".into(),
            value: 1.5,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!(d.node(a).unwrap().params()[0].value, 1.5);
        let before = d.clone();
        assert!(
            Command::SetParam {
                id: a,
                key: "nope".into(),
                value: 1.0
            }
            .apply(&mut d)
            .is_err()
        );
        assert_eq!(d, before);
    }
}
