//! Canvas geometry: crop (with straighten) and image size.
//!
//! Pixel nodes only move: their placements change and their source pixels
//! are untouched, so cropping and resizing are lossless and fully reversible.
//! Document-space masks (on non-pixel nodes, and the selection) have no
//! placement, so they are resampled.

use crate::document::Document;
use crate::node::{NodeId, NodeKind};
use emulsion_raster::{IRect, Mask, Raster};
use glam::{DAffine2, dvec2};
use std::sync::Arc;

/// Tight source-space bounds for sparse drawing/text pixels. Solid source
/// images use their full extent; a local mask can narrow that extent.
fn ink_bounds(raster: &Raster, mask: Option<&Mask>) -> IRect {
    if raster.fill()[3] > 0 {
        return mask.map_or_else(
            || raster.bounds(),
            |m| {
                raster
                    .bounds()
                    .intersect(&emulsion_raster::select::bounds(m))
            },
        );
    }
    let mut bounds = IRect::default();
    let tile = emulsion_raster::TILE as i32;
    for (coord, pixels) in raster.base_tiles() {
        for (i, pixel) in pixels.iter().enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            let (x, y) = (
                coord.x * tile + i as i32 % tile,
                coord.y * tile + i as i32 / tile,
            );
            if x < 0 || y < 0 || x >= raster.width() as i32 || y >= raster.height() as i32 {
                continue;
            }
            if mask.is_some_and(|m| m.get(x as u32, y as u32) == 0) {
                continue;
            }
            bounds = bounds.union(&IRect::new(x, y, 1, 1));
        }
    }
    bounds
}

fn mapped_bounds(rect: IRect, transform: DAffine2) -> IRect {
    if rect.is_empty() {
        return rect;
    }
    let points = [
        dvec2(rect.x as f64, rect.y as f64),
        dvec2(rect.right() as f64, rect.y as f64),
        dvec2(rect.x as f64, rect.bottom() as f64),
        dvec2(rect.right() as f64, rect.bottom() as f64),
    ]
    .map(|p| transform.transform_point2(p));
    let (mut lo, mut hi) = (points[0], points[0]);
    for point in points {
        lo = lo.min(point);
        hi = hi.max(point);
    }
    let (x, y) = ((lo.x + 1e-9).floor() as i32, (lo.y + 1e-9).floor() as i32);
    IRect::new(
        x,
        y,
        ((hi.x - 1e-9).ceil() as i64 - x as i64).clamp(0, i32::MAX as i64) as i32,
        ((hi.y - 1e-9).ceil() as i64 - y as i64).clamp(0, i32::MAX as i64) as i32,
    )
}

/// Content bounds of a selected node or group in document pixels. Sparse
/// paint layers pivot around their marks rather than their canvas-sized buffer.
/// Hidden content is included because it must move with the selected subtree.
pub fn node_bounds(doc: &Document, id: NodeId) -> Option<IRect> {
    let node = doc.node(id)?;
    let mask = node.mask_enabled.then_some(node.mask.as_deref()).flatten();
    let bounds = match &node.kind {
        NodeKind::Raster { raster, placement } => {
            return Some(mapped_bounds(
                ink_bounds(raster, mask),
                placement.to_doc(raster.width(), raster.height()),
            ))
            .filter(|b| !b.is_empty());
        }
        NodeKind::Smart {
            source,
            placement,
            cache,
            offset,
            ..
        } => {
            let cache_mask = Document::composite_mask(node);
            let p = crate::smart::cache_placement(
                placement,
                (source.width(), source.height()),
                (cache.width(), cache.height()),
                *offset,
            );
            return Some(mapped_bounds(
                ink_bounds(cache, cache_mask.as_deref()),
                p.to_doc(cache.width(), cache.height()),
            ))
            .filter(|b| !b.is_empty());
        }
        NodeKind::Path { path, style, .. } => {
            // Path::bounds includes raster allocation padding and an extra
            // pixel; that would offset a rotation pivot by half a pixel.
            let mut points = path.flatten(0.1).into_iter().flat_map(|(points, _)| points);
            let first = points.next()?;
            let (mut lo, mut hi) = (dvec2(first.0, first.1), dvec2(first.0, first.1));
            for (x, y) in points {
                let p = dvec2(x, y);
                lo = lo.min(p);
                hi = hi.max(p);
            }
            let pad = if style.stroke.is_some() {
                style.width as f64 / 2.0
            } else {
                0.0
            };
            lo -= dvec2(pad, pad);
            hi += dvec2(pad, pad);
            let (x, y) = ((lo.x + 1e-9).floor() as i32, (lo.y + 1e-9).floor() as i32);
            IRect::new(
                x,
                y,
                ((hi.x - 1e-9).ceil() as i32 - x).max(1),
                ((hi.y - 1e-9).ceil() as i32 - y).max(1),
            )
        }
        NodeKind::Text { spec, .. } => crate::text::bounds(spec),
        NodeKind::Group { .. } => doc
            .children(Some(id))
            .into_iter()
            .filter_map(|child| node_bounds(doc, child))
            .fold(IRect::default(), |a, b| a.union(&b)),
        NodeKind::Fill { .. } => IRect::new(0, 0, doc.width as i32, doc.height as i32),
        NodeKind::Adjust(_) => mask
            .map(emulsion_raster::select::bounds)
            .unwrap_or_default(),
    };
    let bounds = mask.map_or(bounds, |m| {
        bounds.intersect(&emulsion_raster::select::bounds(m))
    });
    (!bounds.is_empty()).then_some(bounds)
}

/// Align using artwork bounds, retaining editable geometry and source pixels.
/// Whole-pixel offsets retain an existing fractional placement. Center ties
/// use nearest-even rounding: a residual half pixel rounds to zero on the next
/// invocation, so repeated centering never oscillates between adjacent pixels.
pub(crate) fn align_node(
    doc: &mut Document,
    id: NodeId,
    alignment: crate::command::Alignment,
    target: crate::command::AlignTarget,
) -> Result<(), crate::CommandError> {
    use crate::CommandError;
    use crate::command::{AlignTarget, Alignment};
    let node = doc.node(id).ok_or(CommandError::NoSuchNode(id))?;
    if matches!(node.kind, NodeKind::Fill { .. } | NodeKind::Adjust(_)) && node.mask.is_none() {
        return Err(CommandError::NothingToMove(id));
    }
    let reference = match target {
        AlignTarget::Canvas => IRect::new(0, 0, doc.width as i32, doc.height as i32),
        AlignTarget::Selection => doc
            .selection
            .as_deref()
            .map(emulsion_raster::select::bounds)
            .filter(|bounds| !bounds.is_empty())
            .ok_or(CommandError::EmptyAlignmentSelection)?,
    };
    let bounds = node_bounds(doc, id)
        .or_else(|| {
            // Masks can hide every pixel without removing editable geometry.
            // Measure the underlying object only when visible bounds are empty.
            let mut unmasked = doc.clone();
            for node in &mut unmasked.nodes {
                node.mask_enabled = false;
            }
            node_bounds(&unmasked, id)
        })
        .ok_or(CommandError::NothingToMove(id))?;
    let (dx, dy) = match alignment {
        Alignment::Left => ((reference.x as f64 - bounds.x as f64), 0.0),
        Alignment::HorizontalCenter => (
            reference.x as f64 + reference.w as f64 / 2.0 - bounds.x as f64 - bounds.w as f64 / 2.0,
            0.0,
        ),
        Alignment::Right => ((reference.right() as f64 - bounds.right() as f64), 0.0),
        Alignment::Top => (0.0, reference.y as f64 - bounds.y as f64),
        Alignment::VerticalCenter => (
            0.0,
            reference.y as f64 + reference.h as f64 / 2.0 - bounds.y as f64 - bounds.h as f64 / 2.0,
        ),
        Alignment::Bottom => (0.0, reference.bottom() as f64 - bounds.bottom() as f64),
    };
    translate_node(doc, id, dx.round_ties_even(), dy.round_ties_even())
}

/// Tight bounds of stored mask detail, including black holes in white masks.
/// Outside the canvas a mask evaluates to its fill, so only deviations from
/// that fill must remain inside the stored extent to avoid throwing detail away.
fn mask_detail_bounds(mask: &Mask) -> IRect {
    let mut bounds = IRect::default();
    let tile = emulsion_raster::TILE as i32;
    for (coord, pixels) in mask.base_tiles() {
        for (i, pixel) in pixels.iter().enumerate() {
            if *pixel == mask.fill() {
                continue;
            }
            let x = coord.x * tile + i as i32 % tile;
            let y = coord.y * tile + i as i32 / tile;
            if x >= 0 && y >= 0 && x < mask.width() as i32 && y < mask.height() as i32 {
                bounds = bounds.union(&IRect::new(x, y, 1, 1));
            }
        }
    }
    bounds
}

/// Move spatial content, including hidden descendants, without rasterizing
/// editable nodes or resampling raster sources and their local masks.
pub(crate) fn translate_node(
    doc: &mut Document,
    id: NodeId,
    dx: f64,
    dy: f64,
) -> Result<(), crate::CommandError> {
    use crate::{CommandError, DocumentError};
    if !dx.is_finite() || !dy.is_finite() {
        return Err(DocumentError::BadValue(id, "translation").into());
    }
    doc.node(id).ok_or(CommandError::NoSuchNode(id))?;
    if dx == 0.0 && dy == 0.0 {
        return Ok(());
    }
    let ids: std::collections::HashSet<_> = doc.subtree(id).into_iter().collect();
    let movable = doc
        .nodes
        .iter()
        .filter(|n| ids.contains(&n.id))
        .any(|n| match &n.kind {
            NodeKind::Raster { .. } | NodeKind::Smart { .. } | NodeKind::Text { .. } => true,
            NodeKind::Path { path, .. } => path.anchor_count() > 0,
            NodeKind::Fill { .. } | NodeKind::Adjust(_) => n.mask.is_some(),
            NodeKind::Group { .. } => false,
        });
    if !movable {
        return Err(CommandError::NothingToMove(id));
    }
    let (w, h) = (doc.width, doc.height);
    for node in doc.nodes.iter().filter(|n| ids.contains(&n.id)) {
        if matches!(node.kind, NodeKind::Raster { .. } | NodeKind::Smart { .. }) {
            continue;
        }
        if let Some(mask) = &node.mask {
            let bounds = mask_detail_bounds(mask);
            // Floor/ceil include the extra edge samples introduced by a
            // fractional bilinear translation. Disabled masks retain their
            // detail too, so enabling one later never exposes a clipped mask.
            if !bounds.is_empty()
                && ((bounds.x as f64 + dx).floor() < 0.0
                    || (bounds.y as f64 + dy).floor() < 0.0
                    || (bounds.right() as f64 + dx).ceil() > w as f64
                    || (bounds.bottom() as f64 + dy).ceil() > h as f64)
            {
                return Err(CommandError::MaskWouldClip(node.id));
            }
        }
    }
    let inverse = DAffine2::from_translation(dvec2(-dx, -dy));
    for node in &mut doc.nodes {
        if !ids.contains(&node.id) {
            continue;
        }
        match &mut node.kind {
            NodeKind::Raster { placement, .. } | NodeKind::Smart { placement, .. } => {
                placement.x += dx;
                placement.y += dy;
                // Source-space masks move through placement with the pixels.
                continue;
            }
            NodeKind::Path { path, style, cache } => {
                let mut updated = (**path).clone();
                updated.translate(dx, dy);
                if updated.subpaths.iter().flat_map(|p| &p.anchors).any(|a| {
                    [a.p, a.h_in, a.h_out]
                        .iter()
                        .any(|p| !p.0.is_finite() || !p.1.is_finite())
                }) {
                    return Err(DocumentError::BadValue(id, "translation").into());
                }
                *cache = Arc::new(updated.rasterize(style, w, h));
                *path = Arc::new(updated);
            }
            NodeKind::Text { spec, cache } => {
                let mut updated = (**spec).clone();
                updated.x = (updated.x as f64 + dx) as f32;
                updated.y = (updated.y as f64 + dy) as f32;
                if !updated.x.is_finite() || !updated.y.is_finite() {
                    return Err(DocumentError::BadValue(id, "translation").into());
                }
                *cache = Arc::new(crate::text::rasterize(&updated, w, h));
                *spec = Arc::new(updated);
            }
            NodeKind::Group { .. } | NodeKind::Fill { .. } | NodeKind::Adjust(_) => {}
        }
        if let Some(mask) = &node.mask {
            let translated = if dx.abs() >= w as f64 || dy.abs() >= h as f64 {
                Mask::empty(w, h, mask.fill())
            } else {
                remap(mask, w, h, inverse)
            };
            node.mask = Some(Arc::new(translated));
        }
    }
    Ok(())
}

/// Rotate the selected subtree about one common pivot, retaining editable
/// geometry and text and sharing original raster/smart source buffers.
pub(crate) fn rotate_node(
    doc: &mut Document,
    id: NodeId,
    degrees: f64,
) -> Result<(), crate::CommandError> {
    use crate::{CommandError, DocumentError};
    if !degrees.is_finite() {
        return Err(DocumentError::BadValue(id, "rotation").into());
    }
    doc.node(id).ok_or(CommandError::NoSuchNode(id))?;
    let ids: std::collections::HashSet<_> = doc.subtree(id).into_iter().collect();
    for node in &doc.nodes {
        if node.locked && (ids.contains(&node.id) || doc.is_ancestor(node.id, id)) {
            return Err(CommandError::Locked(node.id));
        }
    }
    let bounds = node_bounds(doc, id).ok_or(CommandError::NothingToRotate(id))?;
    let degrees = degrees % 360.0;
    if degrees == 0.0 {
        return Ok(());
    }
    let pivot = dvec2(
        bounds.x as f64 + bounds.w as f64 / 2.0,
        bounds.y as f64 + bounds.h as f64 / 2.0,
    );
    let transform = DAffine2::from_translation(pivot)
        * DAffine2::from_angle(degrees.to_radians())
        * DAffine2::from_translation(-pivot);
    let inverse = transform.inverse();
    let (w, h) = (doc.width, doc.height);
    for node in &mut doc.nodes {
        if !ids.contains(&node.id) {
            continue;
        }
        match &mut node.kind {
            NodeKind::Raster { raster, placement }
            | NodeKind::Smart {
                source: raster,
                placement,
                ..
            } => {
                let half = dvec2(
                    raster.width() as f64 * placement.scale_x / 2.0,
                    raster.height() as f64 * placement.scale_y / 2.0,
                );
                let centre = transform.transform_point2(dvec2(placement.x, placement.y) + half);
                placement.x = centre.x - half.x;
                placement.y = centre.y - half.y;
                placement.rotation = (placement.rotation + degrees) % 360.0;
                // Pixel masks live in layer coordinates and follow placement.
                continue;
            }
            NodeKind::Path { path, style, cache } => {
                let mut updated = (**path).clone();
                updated.transform(transform);
                *cache = Arc::new(updated.rasterize(style, w, h));
                *path = Arc::new(updated);
            }
            NodeKind::Text { spec, cache } => {
                let mut updated = (**spec).clone();
                let anchor = transform.transform_point2(dvec2(updated.x as f64, updated.y as f64));
                updated.x = anchor.x as f32;
                updated.y = anchor.y as f32;
                updated.rotation = (updated.rotation as f64 + degrees) as f32;
                let updated = updated.sanitized();
                *cache = Arc::new(crate::text::rasterize(&updated, w, h));
                *spec = Arc::new(updated);
            }
            NodeKind::Group { .. } | NodeKind::Fill { .. } | NodeKind::Adjust(_) => {}
        }
        if let Some(mask) = &node.mask {
            node.mask = Some(Arc::new(remap(mask, w, h, inverse)));
        }
    }
    Ok(())
}

/// Resample a document-space mask: output pixel `p` reads `inv(p)`.
fn remap(m: &Mask, w: u32, h: u32, inv: DAffine2) -> Mask {
    let identity_shift = inv.matrix2 == glam::DMat2::IDENTITY
        && inv.translation.x.fract() == 0.0
        && inv.translation.y.fract() == 0.0;
    if identity_shift {
        let (dx, dy) = (inv.translation.x as i32, inv.translation.y as i32);
        let src = m.read_rect(IRect::new(dx, dy, w as i32, h as i32));
        // read_rect fills outside with the source fill; keep that fill.
        return Mask::from_pixels(w, h, m.fill(), &src);
    }
    let fill = m.fill() as f32;
    let get = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= m.width() as i64 || y >= m.height() as i64 {
            fill
        } else {
            m.get(x as u32, y as u32) as f32
        }
    };
    Mask::from_fn(w, h, m.fill(), |x, y| {
        let p = inv.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5)) - dvec2(0.5, 0.5);
        let (fx, fy) = (p.x.floor(), p.y.floor());
        let (ax, ay) = ((p.x - fx) as f32, (p.y - fy) as f32);
        let (ix, iy) = (fx as i64, fy as i64);
        let top = get(ix, iy) + (get(ix + 1, iy) - get(ix, iy)) * ax;
        let bot = get(ix, iy + 1) + (get(ix + 1, iy + 1) - get(ix, iy + 1)) * ax;
        (top + (bot - top) * ay).round().clamp(0.0, 255.0) as u8
    })
}

/// Apply `to_new` (old document space → new document space) to everything.
fn transform_all(doc: &mut Document, w: u32, h: u32, to_new: DAffine2) {
    let angle = to_new
        .matrix2
        .x_axis
        .y
        .atan2(to_new.matrix2.x_axis.x)
        .to_degrees();
    let scale = to_new.matrix2.x_axis.length();
    let inv = to_new.inverse();
    for n in &mut doc.nodes {
        match &mut n.kind {
            NodeKind::Raster { raster, placement } => {
                // Placements rotate about the content centre: move the centre,
                // add the angle, scale the size.
                let (rw, rh) = (raster.width() as f64, raster.height() as f64);
                let c = dvec2(
                    placement.x + rw * placement.scale_x / 2.0,
                    placement.y + rh * placement.scale_y / 2.0,
                );
                let c2 = to_new.transform_point2(c);
                placement.scale_x *= scale;
                placement.scale_y *= scale;
                placement.rotation += angle;
                placement.x = c2.x - rw * placement.scale_x / 2.0;
                placement.y = c2.y - rh * placement.scale_y / 2.0;
            }
            NodeKind::Smart {
                placement, source, ..
            } => {
                let (rw, rh) = (source.width() as f64, source.height() as f64);
                let c = dvec2(
                    placement.x + rw * placement.scale_x / 2.0,
                    placement.y + rh * placement.scale_y / 2.0,
                );
                let c2 = to_new.transform_point2(c);
                placement.scale_x *= scale;
                placement.scale_y *= scale;
                placement.rotation += angle;
                placement.x = c2.x - rw * placement.scale_x / 2.0;
                placement.y = c2.y - rh * placement.scale_y / 2.0;
            }
            NodeKind::Path { path, style, cache } => {
                let mut p = (**path).clone();
                p.transform(to_new);
                style.width = (style.width as f64 * scale) as f32;
                *cache = Arc::new(p.rasterize(style, w, h));
                *path = Arc::new(p);
                if let Some(m) = &n.mask {
                    n.mask = Some(Arc::new(remap(m, w, h, inv)));
                }
            }
            NodeKind::Text { spec, cache } => {
                let mut s = (**spec).clone();
                let p = to_new.transform_point2(glam::dvec2(s.x as f64, s.y as f64));
                s.x = p.x as f32;
                s.y = p.y as f32;
                s.rotation += angle as f32;
                s.size = (s.size as f64 * scale) as f32;
                s.width = s.width.map(|w| (w as f64 * scale) as f32);
                s.letter_spacing = (s.letter_spacing as f64 * scale) as f32;
                let s = s.sanitized();
                *cache = Arc::new(crate::text::rasterize(&s, w, h));
                *spec = Arc::new(s);
                if let Some(m) = &n.mask {
                    n.mask = Some(Arc::new(remap(m, w, h, inv)));
                }
            }
            _ => {
                if let Some(m) = &n.mask {
                    n.mask = Some(Arc::new(remap(m, w, h, inv)));
                }
            }
        }
    }
    // Guides stay straight only when nothing rotates; otherwise they go.
    if angle.abs() < 1e-9 {
        for g in &mut doc.guides {
            let p = if g.vertical {
                dvec2(g.pos, 0.0)
            } else {
                dvec2(0.0, g.pos)
            };
            let q = to_new.transform_point2(p);
            g.pos = if g.vertical { q.x } else { q.y };
        }
    } else {
        doc.guides.clear();
    }
    if let Some(sel) = &doc.selection {
        doc.selection = Some(Arc::new(remap(sel, w, h, inv)));
    }
    doc.width = w;
    doc.height = h;
}

pub fn crop(doc: &mut Document, rect: IRect, rotation: f64) {
    let c = dvec2(doc.width as f64 / 2.0, doc.height as f64 / 2.0);
    let rot = DAffine2::from_translation(c)
        * DAffine2::from_angle(rotation.to_radians())
        * DAffine2::from_translation(-c);
    let to_new = DAffine2::from_translation(dvec2(-rect.x as f64, -rect.y as f64)) * rot;
    transform_all(doc, rect.w.max(1) as u32, rect.h.max(1) as u32, to_new);
}

/// Trim pixel layers that sit unrotated and unscaled on the canvas down to
/// the part the canvas shows, mask included; returns how many changed.
/// Layers wholly inside stay as they are; transformed and smart layers
/// are left alone, since cutting them would mean resampling.
pub fn trim_to_canvas(doc: &mut Document) -> usize {
    let canvas = IRect::new(0, 0, doc.width as i32, doc.height as i32);
    let mut changed = 0;
    for n in doc.nodes.iter_mut() {
        let NodeKind::Raster { raster, placement } = &n.kind else {
            continue;
        };
        if placement.scale_x != 1.0
            || placement.scale_y != 1.0
            || placement.rotation != 0.0
            || placement.flip_x
            || placement.flip_y
        {
            continue;
        }
        // The canvas in layer pixels, widened to whole pixels so a
        // fractional offset never loses an edge column.
        let (ox, oy) = (placement.x, placement.y);
        let lx0 = (canvas.x as f64 - ox).floor() as i32;
        let ly0 = (canvas.y as f64 - oy).floor() as i32;
        let lx1 = ((canvas.x + canvas.w) as f64 - ox).ceil() as i32;
        let ly1 = ((canvas.y + canvas.h) as f64 - oy).ceil() as i32;
        let keep = IRect::new(lx0, ly0, lx1 - lx0, ly1 - ly0).intersect(&raster.bounds());
        if keep == raster.bounds() {
            continue;
        }
        let (w, h) = (keep.w.max(0) as u32, keep.h.max(0) as u32);
        let mut placement = *placement;
        placement.x += keep.x as f64;
        placement.y += keep.y as f64;
        let cut = if w == 0 || h == 0 {
            // Nothing left on the canvas: a single transparent pixel where it was.
            placement.x = ox;
            placement.y = oy;
            Raster::transparent(1, 1)
        } else {
            Raster::from_pixels(w, h, raster.fill(), &raster.read_rect(keep))
        };
        let mask = n.mask.as_ref().map(|m| {
            if m.width() == raster.width() && m.height() == raster.height() && w > 0 && h > 0 {
                Arc::new(emulsion_raster::Mask::from_pixels(
                    w,
                    h,
                    m.fill(),
                    &m.read_rect(keep),
                ))
            } else {
                m.clone()
            }
        });
        n.kind = NodeKind::Raster {
            raster: Arc::new(cut),
            placement,
        };
        n.mask = mask;
        changed += 1;
    }
    changed
}

pub fn resize(doc: &mut Document, width: u32, height: u32) {
    // Uniform scale by width keeps rotated placements exact.
    let s = width as f64 / doc.width as f64;
    transform_all(
        doc,
        width.max(1),
        height.max(1),
        DAffine2::from_scale(dvec2(s, s)),
    );
}

#[cfg(test)]
mod trim_tests {
    use crate::command::Slot;
    use crate::{Command, Document, Node, NodeKind};
    use emulsion_raster::composite::flatten;
    use emulsion_raster::{IRect, Placement, Raster};
    use std::sync::Arc;

    #[test]
    fn crop_then_trim_cuts_layers_and_masks_but_keeps_the_picture() {
        let mut d = Document::new(200, 100);
        let r = Raster::from_fn(200, 100, [0; 4], |x, _| {
            if x < 100 {
                [65535, 0, 0, 65535]
            } else {
                [0, 0, 65535, 65535]
            }
        });
        let mut n = Node::raster(0, "img", Arc::new(r), Placement::default());
        n.mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
            200,
            100,
            255,
            |x, _| if x < 150 { 255 } else { 0 },
        )));
        n.mask_enabled = true;
        Command::AddNode {
            node: Box::new(n),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        // A rotated layer must be left alone.
        let mut rotated = Node::raster(
            0,
            "tilted",
            Arc::new(Raster::solid(40, 40, [0.0, 1.0, 0.0, 1.0])),
            Placement::default(),
        );
        if let NodeKind::Raster { placement, .. } = &mut rotated.kind {
            placement.rotation = 15.0;
        }
        Command::AddNode {
            node: Box::new(rotated),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();

        Command::Crop {
            rect: IRect::new(80, 20, 100, 60),
            rotation: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        let before = flatten(&d.composite_tree(), 0);
        let n = Command::TrimToCanvas.apply(&mut d);
        assert!(n.is_ok());
        let after = flatten(&d.composite_tree(), 0);
        assert_eq!(
            before.to_srgba8(),
            after.to_srgba8(),
            "the picture is unchanged"
        );

        let NodeKind::Raster { raster, placement } = &d.nodes[0].kind else {
            panic!("raster");
        };
        assert_eq!(
            (raster.width(), raster.height()),
            (100, 60),
            "cut to the canvas"
        );
        assert_eq!((placement.x, placement.y), (0.0, 0.0));
        let m = d.nodes[0].mask.as_ref().unwrap();
        assert_eq!((m.width(), m.height()), (100, 60), "mask cut with it");
        // Old x=150 is new x=70: the mask edge survives in place.
        assert_eq!(m.get(69, 10), 255);
        assert_eq!(m.get(70, 10), 0);
        let NodeKind::Raster { raster, .. } = &d.nodes[1].kind else {
            panic!("raster");
        };
        assert_eq!(
            (raster.width(), raster.height()),
            (40, 40),
            "rotated layer untouched"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::node_bounds;
    use crate::command::Slot;
    use crate::{Command, Document, Node, NodeKind};
    use emulsion_raster::composite::flatten;
    use emulsion_raster::{IRect, Placement, Raster};
    use std::sync::Arc;

    fn doc() -> Document {
        let mut d = Document::new(200, 100);
        let r = Raster::from_fn(200, 100, [0; 4], |x, y| {
            if x < 100 {
                [65535, 0, 0, 65535]
            } else if y < 50 {
                [0, 65535, 0, 65535]
            } else {
                [0, 0, 65535, 65535]
            }
        });
        Command::AddNode {
            node: Box::new(Node::raster(0, "img", Arc::new(r), Placement::default())),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        d
    }

    fn alignment_doc() -> (Document, crate::NodeId, Arc<Raster>) {
        let mut doc = Document::new(100, 80);
        let source = Arc::new(Raster::solid(20, 10, [1.0, 0.0, 0.0, 1.0]));
        let id = Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Align",
                source.clone(),
                Placement::at(15.0, 20.0),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        (doc, id, source)
    }

    #[test]
    fn six_alignments_use_canvas_or_selection_bounds_and_repeat_without_history() {
        use crate::command::{AlignTarget, Alignment::*};
        let (baseline, id, source) = alignment_doc();
        for (target, cases) in [
            (
                AlignTarget::Canvas,
                [
                    (Left, 0.0, 20.0),
                    (HorizontalCenter, 40.0, 20.0),
                    (Right, 80.0, 20.0),
                    (Top, 15.0, 0.0),
                    (VerticalCenter, 15.0, 35.0),
                    (Bottom, 15.0, 70.0),
                ],
            ),
            (
                AlignTarget::Selection,
                [
                    (Left, 20.0, 20.0),
                    (HorizontalCenter, 35.0, 20.0),
                    (Right, 50.0, 20.0),
                    (Top, 15.0, 15.0),
                    (VerticalCenter, 15.0, 30.0),
                    (Bottom, 15.0, 45.0),
                ],
            ),
        ] {
            for (alignment, x, y) in cases {
                let mut doc = baseline.clone();
                doc.selection = Some(Arc::new(emulsion_raster::select::rect(
                    100, 80, 20.0, 15.0, 50.0, 40.0,
                )));
                let selected_area = doc.selection.clone();
                let before = doc.clone();
                let mut editor = crate::Editor::new(doc, None);
                let command = Command::AlignNode {
                    id,
                    alignment,
                    target,
                };
                editor.execute(command.clone()).unwrap();
                let NodeKind::Raster { raster, placement } = &editor.doc.node(id).unwrap().kind
                else {
                    panic!("raster")
                };
                assert_eq!(
                    (placement.x, placement.y),
                    (x, y),
                    "{alignment:?} {target:?}"
                );
                assert!(Arc::ptr_eq(raster, &source));
                assert!(Arc::ptr_eq(
                    editor.doc.selection.as_ref().unwrap(),
                    selected_area.as_ref().unwrap()
                ));
                let revision = editor.revision;
                editor.execute(command).unwrap();
                assert_eq!(editor.revision, revision);
                assert_eq!(editor.history.len(), 1);
                assert!(editor.undo());
                assert_eq!(editor.doc, before);
            }
        }
    }

    #[test]
    fn center_alignment_retains_fractional_placement_and_is_stable_at_half_pixel_ties() {
        use crate::command::{AlignTarget, Alignment};
        for width in [9, 10] {
            for x in [10.0, 10.25, 11.25] {
                let mut doc = Document::new(100, 80);
                let id = Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        "Fractional",
                        Arc::new(Raster::solid(width, 7, [1.0; 4])),
                        Placement::at(x, 12.25),
                    )),
                    slot: Slot::TOP,
                }
                .apply(&mut doc)
                .unwrap()
                .unwrap();
                let mut editor = crate::Editor::new(doc, None);
                for alignment in [Alignment::HorizontalCenter, Alignment::VerticalCenter] {
                    let cmd = Command::AlignNode {
                        id,
                        alignment,
                        target: AlignTarget::Canvas,
                    };
                    editor.execute(cmd.clone()).unwrap();
                    let first = editor.doc.clone();
                    let steps = editor.history.len();
                    let revision = editor.revision;
                    for _ in 0..3 {
                        editor.execute(cmd.clone()).unwrap();
                    }
                    assert_eq!(editor.doc, first);
                    assert_eq!(editor.history.len(), steps);
                    assert_eq!(editor.revision, revision);
                }
                let NodeKind::Raster { placement, .. } = &editor.doc.node(id).unwrap().kind else {
                    panic!("raster")
                };
                assert_eq!(placement.x.fract(), x.fract());
                assert_eq!(placement.y.fract(), 0.25);
                let bounds = node_bounds(&editor.doc, id).unwrap();
                assert!((bounds.x as f64 + bounds.w as f64 / 2.0 - 50.0).abs() <= 0.5);
                assert!((bounds.y as f64 + bounds.h as f64 / 2.0 - 40.0).abs() <= 0.5);
            }
        }
    }

    #[test]
    fn alignment_rejects_absent_selection_and_uniform_nonspatial_layers() {
        use crate::command::{AlignTarget, Alignment};
        let (mut doc, id, _) = alignment_doc();
        for selection in [
            None,
            Some(Arc::new(emulsion_raster::Mask::empty(100, 80, 0))),
        ] {
            doc.selection = selection;
            let before = doc.clone();
            assert_eq!(
                Command::AlignNode {
                    id,
                    alignment: Alignment::Left,
                    target: AlignTarget::Selection
                }
                .apply(&mut doc),
                Err(crate::CommandError::EmptyAlignmentSelection)
            );
            assert_eq!(doc, before);
        }
        for kind in [
            NodeKind::Fill {
                rgba: [0, 0, 0, 255],
            },
            NodeKind::Adjust(emulsion_raster::Adjustment::Exposure {
                exposure: 1.0,
                offset: 0.0,
                gamma: 1.0,
            }),
        ] {
            let id = Command::AddNode {
                node: Box::new(Node::new(0, "Uniform", kind)),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap()
            .unwrap();
            let before = doc.clone();
            assert_eq!(
                Command::AlignNode {
                    id,
                    alignment: Alignment::Left,
                    target: AlignTarget::Canvas
                }
                .apply(&mut doc),
                Err(crate::CommandError::NothingToMove(id))
            );
            assert_eq!(doc, before);
        }
    }

    #[test]
    fn alignment_moves_masked_group_as_editable_unit_and_protects_locks_and_masks() {
        use crate::command::{AlignTarget, Alignment};
        use emulsion_raster::vector::{Path, PathStyle};
        let (mut doc, raster_id, source) = alignment_doc();
        let path = Arc::new(Path::from_svg("M 40 20 L 60 20 L 60 30 Z").unwrap());
        let path_id = Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Path",
                path.clone(),
                PathStyle {
                    stroke: None,
                    fill: Some([0, 0, 0, 255]),
                    ..Default::default()
                },
                100,
                80,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let group = Command::Group {
            ids: vec![raster_id, path_id],
            name: "Align group".into(),
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        // Fully hidden groups still expose their underlying editable geometry.
        Command::SetMask {
            id: group,
            mask: Some(Arc::new(emulsion_raster::Mask::empty(100, 80, 0))),
        }
        .apply(&mut doc)
        .unwrap();
        let before = doc.clone();
        Command::AlignNode {
            id: group,
            alignment: Alignment::Left,
            target: AlignTarget::Canvas,
        }
        .apply(&mut doc)
        .unwrap();
        let NodeKind::Raster { raster, placement } = &doc.node(raster_id).unwrap().kind else {
            panic!("raster")
        };
        assert!(Arc::ptr_eq(raster, &source));
        assert_eq!(placement.x, 0.0);
        let NodeKind::Path {
            path: translated, ..
        } = &doc.node(path_id).unwrap().kind
        else {
            panic!("path")
        };
        assert_eq!(translated.subpaths[0].anchors[0].p, (25.0, 20.0));
        assert_eq!(path.subpaths[0].anchors[0].p, (40.0, 20.0));
        for locked in [group, path_id] {
            doc = before.clone();
            Command::SetLocked {
                id: locked,
                locked: true,
            }
            .apply(&mut doc)
            .unwrap();
            let protected = doc.clone();
            assert!(matches!(
                Command::AlignNode {
                    id: group,
                    alignment: Alignment::Right,
                    target: AlignTarget::Canvas
                }
                .apply(&mut doc),
                Err(crate::CommandError::Locked(_))
            ));
            assert_eq!(doc, protected);
        }
        doc = before;
        // Visible artwork starts at x=15, but the group's mask stores detail
        // starting at x=5. Aligning artwork to x=0 would discard mask detail.
        Command::SetMask {
            id: group,
            mask: Some(Arc::new(emulsion_raster::select::rect(
                100, 80, 5.0, 5.0, 80.0, 60.0,
            ))),
        }
        .apply(&mut doc)
        .unwrap();
        let protected = doc.clone();
        assert_eq!(
            Command::AlignNode {
                id: group,
                alignment: Alignment::Left,
                target: AlignTarget::Canvas
            }
            .apply(&mut doc),
            Err(crate::CommandError::MaskWouldClip(group))
        );
        assert_eq!(doc, protected);
    }

    #[test]
    fn translation_moves_mixed_group_hidden_children_and_masks_without_baking_sources() {
        use emulsion_raster::vector::{Path, PathStyle};
        let mut d = Document::new(128, 96);
        let add = |d: &mut Document, node: Node, parent| {
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::top_of(parent),
            }
            .apply(d)
            .unwrap()
            .unwrap()
        };
        let mut group = Node::group(0, "Move together");
        group.mask = Some(Arc::new(emulsion_raster::select::rect(
            128, 96, 10.0, 10.0, 80.0, 60.0,
        )));
        let group = add(&mut d, group, None);
        let source = Arc::new(Raster::solid(8, 6, [1.0, 0.0, 0.0, 1.0]));
        let local_mask = Arc::new(emulsion_raster::Mask::white(8, 6));
        let mut raster = Node::raster(0, "Pixels", source.clone(), Placement::at(20.0, 20.0));
        raster.mask = Some(local_mask.clone());
        let raster = add(&mut d, raster, Some(group));
        let mut smart = Node::smart(
            0,
            "Hidden smart",
            source.clone(),
            Vec::new(),
            Placement::at(40.0, 20.0),
        );
        smart.visible = false;
        smart.mask = Some(local_mask.clone());
        let smart = add(&mut d, smart, Some(group));
        let path = Arc::new(Path::from_svg("M 20 40 L 40 40 L 40 50 Z").unwrap());
        let mut path_node = Node::path(
            0,
            "Editable path",
            path.clone(),
            PathStyle::default(),
            128,
            96,
        );
        path_node.mask = Some(Arc::new(emulsion_raster::select::rect(
            128, 96, 20.0, 40.0, 20.0, 10.0,
        )));
        let path_id = add(&mut d, path_node, Some(group));
        let text = add(
            &mut d,
            Node::text(
                0,
                "Editable text",
                crate::text::TextSpec {
                    text: "Move".into(),
                    x: 20.0,
                    y: 55.0,
                    size: 12.0,
                    rotation: 15.0,
                    ..Default::default()
                },
                128,
                96,
            ),
            Some(group),
        );
        let before = d.clone();
        let mut editor = crate::Editor::new(d, None);
        editor
            .execute(Command::TranslateNode {
                id: group,
                dx: 3.0,
                dy: 4.0,
            })
            .unwrap();
        let moved = &editor.doc;
        let NodeKind::Raster {
            raster: pixels,
            placement,
        } = &moved.node(raster).unwrap().kind
        else {
            panic!("raster")
        };
        assert!(Arc::ptr_eq(pixels, &source));
        assert_eq!((placement.x, placement.y), (23.0, 24.0));
        assert!(Arc::ptr_eq(
            moved.node(raster).unwrap().mask.as_ref().unwrap(),
            &local_mask
        ));
        let NodeKind::Smart {
            source: pixels,
            placement,
            ..
        } = &moved.node(smart).unwrap().kind
        else {
            panic!("smart")
        };
        assert!(Arc::ptr_eq(pixels, &source));
        assert_eq!((placement.x, placement.y), (43.0, 24.0));
        assert!(!moved.node(smart).unwrap().visible);
        assert!(Arc::ptr_eq(
            moved.node(smart).unwrap().mask.as_ref().unwrap(),
            &local_mask
        ));
        let NodeKind::Path {
            path: moved_path, ..
        } = &moved.node(path_id).unwrap().kind
        else {
            panic!("path")
        };
        assert_eq!(moved_path.subpaths[0].anchors[0].p, (23.0, 44.0));
        assert_eq!(path.subpaths[0].anchors[0].p, (20.0, 40.0));
        assert_eq!(
            moved
                .node(path_id)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .get(23, 44),
            255
        );
        assert_eq!(
            moved
                .node(group)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .get(10, 10),
            0
        );
        assert_eq!(
            moved
                .node(group)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .get(13, 14),
            255
        );
        let NodeKind::Text { spec, .. } = &moved.node(text).unwrap().kind else {
            panic!("text")
        };
        assert_eq!((spec.x, spec.y, spec.rotation), (23.0, 59.0, 15.0));
        assert_eq!(spec.text, "Move");
        assert_eq!(editor.history.len(), 1);
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
    }

    #[test]
    fn translation_rejects_invalid_locked_or_nonspatial_targets_atomically() {
        let mut d = doc();
        let id = d.nodes[0].id;
        let group = Command::Group {
            ids: vec![id],
            name: "Group".into(),
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        for (dx, dy) in [(f64::NAN, 0.0), (0.0, f64::INFINITY)] {
            let before = d.clone();
            assert!(
                Command::TranslateNode { id: group, dx, dy }
                    .apply(&mut d)
                    .is_err()
            );
            assert_eq!(d, before);
        }
        for lock in [id, group] {
            Command::SetLocked {
                id: lock,
                locked: true,
            }
            .apply(&mut d)
            .unwrap();
            let before = d.clone();
            for target in [id, group] {
                assert!(matches!(
                    Command::TranslateNode {
                        id: target,
                        dx: 2.0,
                        dy: 3.0
                    }
                    .apply(&mut d),
                    Err(crate::CommandError::Locked(_))
                ));
                assert_eq!(d, before);
            }
            Command::SetLocked {
                id: lock,
                locked: false,
            }
            .apply(&mut d)
            .unwrap();
        }
        for node in [
            Node::group(0, "Empty"),
            Node::new(
                0,
                "Uniform",
                NodeKind::Fill {
                    rgba: [0, 0, 0, 255],
                },
            ),
            Node::adjust(
                0,
                emulsion_raster::Adjustment::Exposure {
                    exposure: 1.0,
                    offset: 0.0,
                    gamma: 1.0,
                },
            ),
        ] {
            let id = Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut d)
            .unwrap()
            .unwrap();
            let before = d.clone();
            assert!(matches!(
                Command::TranslateNode {
                    id,
                    dx: 1.0,
                    dy: 0.0
                }
                .apply(&mut d),
                Err(crate::CommandError::NothingToMove(_))
            ));
            assert_eq!(d, before);
        }
    }

    #[test]
    fn translation_moves_masked_fill_and_adjustment_in_document_coordinates() {
        let mut d = Document::new(32, 24);
        for kind in [
            NodeKind::Fill {
                rgba: [0, 0, 0, 255],
            },
            NodeKind::Adjust(emulsion_raster::Adjustment::Exposure {
                exposure: 1.0,
                offset: 0.0,
                gamma: 1.0,
            }),
        ] {
            let mut node = Node::new(0, "Masked", kind);
            node.mask = Some(Arc::new(emulsion_raster::select::rect(
                32, 24, 5.0, 6.0, 8.0, 4.0,
            )));
            let id = Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut d)
            .unwrap()
            .unwrap();
            Command::TranslateNode {
                id,
                dx: 3.0,
                dy: -2.0,
            }
            .apply(&mut d)
            .unwrap();
            assert_eq!(
                emulsion_raster::select::bounds(d.node(id).unwrap().mask.as_ref().unwrap()),
                IRect::new(8, 4, 8, 4)
            );
        }
    }

    #[test]
    fn translation_protects_positive_mask_detail_and_white_mask_holes_including_disabled_masks() {
        use emulsion_raster::Mask;
        for fill in [0, 255] {
            let mut d = Document::new(16, 12);
            let mask = Arc::new(Mask::from_fn(16, 12, fill, |x, y| {
                if (2..6).contains(&x) && (3..7).contains(&y) {
                    255 - fill
                } else {
                    fill
                }
            }));
            let mut node = Node::new(
                1,
                "Mask detail",
                NodeKind::Fill {
                    rgba: [255, 0, 0, 255],
                },
            );
            node.mask = Some(mask.clone());
            node.mask_enabled = fill == 0;
            d.nodes.push(node);
            let before = d.clone();
            for (dx, dy) in [(-2.25, 0.0), (10.25, 0.0), (0.0, -3.25), (0.0, 5.25)] {
                let error = Command::TranslateNode { id: 1, dx, dy }
                    .apply(&mut d)
                    .unwrap_err();
                assert_eq!(error, crate::CommandError::MaskWouldClip(1));
                assert_eq!(
                    error.to_string(),
                    "This move would clip a document-space mask; enlarge the canvas first."
                );
                assert_eq!(d, before);
                assert!(Arc::ptr_eq(d.nodes[0].mask.as_ref().unwrap(), &mask));
            }
            // Fractional interpolation wholly inside the canvas is allowed.
            Command::TranslateNode {
                id: 1,
                dx: -1.5,
                dy: -2.5,
            }
            .apply(&mut d)
            .unwrap();
            let shifted = d.nodes[0].mask.as_ref().unwrap();
            assert_eq!(shifted.get(0, 0), if fill == 0 { 64 } else { 191 });
            assert_eq!(d.nodes[0].mask_enabled, fill == 0);
            // An exact integer translation may put detail flush with an edge.
            d = before;
            Command::TranslateNode {
                id: 1,
                dx: -2.0,
                dy: -3.0,
            }
            .apply(&mut d)
            .unwrap();
            assert_eq!(d.nodes[0].mask.as_ref().unwrap().get(0, 0), 255 - fill);
        }
    }

    #[test]
    fn group_mask_clipping_rejects_entire_move_and_local_pixel_masks_can_leave_canvas() {
        use emulsion_raster::Mask;
        let mut d = Document::new(16, 12);
        let source = Arc::new(Raster::solid(4, 4, [1.0, 0.0, 0.0, 1.0]));
        let local = Arc::new(Mask::white(4, 4));
        let mut raster = Node::raster(1, "Pixels", source.clone(), Placement::at(2.0, 3.0));
        raster.parent = Some(2);
        raster.mask = Some(local.clone());
        let mut group = Node::group(2, "Group mask");
        group.mask = Some(Arc::new(emulsion_raster::select::rect(
            16, 12, 2.0, 3.0, 4.0, 4.0,
        )));
        group.mask_enabled = false;
        d.nodes = vec![raster, group];
        let before = d.clone();
        assert_eq!(
            Command::TranslateNode {
                id: 2,
                dx: 11.0,
                dy: 0.0
            }
            .apply(&mut d),
            Err(crate::CommandError::MaskWouldClip(2))
        );
        assert_eq!(d, before);
        // Moving the raster alone carries its local mask without clipping it.
        Command::TranslateNode {
            id: 1,
            dx: 20.0,
            dy: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        let NodeKind::Raster { raster, placement } = &d.nodes[0].kind else {
            panic!("raster")
        };
        assert_eq!(placement.x, 22.0);
        assert!(Arc::ptr_eq(raster, &source));
        assert!(Arc::ptr_eq(d.nodes[0].mask.as_ref().unwrap(), &local));
        Command::TranslateNode {
            id: 1,
            dx: -20.0,
            dy: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!(d, before);
    }

    #[test]
    fn rotate_sparse_drawing_uses_ink_centre_and_undo_preserves_sources() {
        let mut d = Document::new(200, 100);
        let source = Arc::new(Raster::from_fn(200, 100, [0; 4], |x, y| {
            if (20..60).contains(&x) && (30..40).contains(&y) {
                [65535, 0, 0, 65535]
            } else {
                [0; 4]
            }
        }));
        d.nodes.push(Node::raster(
            1,
            "drawing",
            source.clone(),
            Placement::default(),
        ));
        assert_eq!(node_bounds(&d, 1), Some(IRect::new(20, 30, 40, 10)));
        let mut e = crate::Editor::new(d, None);
        e.execute(Command::RotateNode {
            id: 1,
            degrees: 90.0,
        })
        .unwrap();
        assert_eq!(node_bounds(&e.doc, 1), Some(IRect::new(35, 15, 10, 40)));
        let NodeKind::Raster { raster, placement } = &e.doc.node(1).unwrap().kind else {
            panic!()
        };
        assert!(Arc::ptr_eq(&source, raster));
        assert_eq!(placement.rotation, 90.0);
        let rotated = *placement;
        assert!(e.undo());
        assert_eq!(node_bounds(&e.doc, 1), Some(IRect::new(20, 30, 40, 10)));
        assert!(e.redo());
        let NodeKind::Raster { raster, placement } = &e.doc.node(1).unwrap().kind else {
            panic!()
        };
        assert!(Arc::ptr_eq(&source, raster));
        assert_eq!(*placement, rotated);
    }

    #[test]
    fn rotation_preserves_scaled_flipped_raster_and_smart_geometry() {
        for smart in [false, true] {
            let mut d = Document::new(300, 300);
            let source = Arc::new(Raster::solid(40, 20, [1.0, 0.0, 0.0, 1.0]));
            let placement = Placement {
                x: 100.0,
                y: 80.0,
                scale_x: 1.7,
                scale_y: 0.8,
                rotation: 23.0,
                flip_x: true,
                flip_y: false,
            };
            let mut node = Node::raster(1, "object", source.clone(), placement);
            if smart {
                node.kind = NodeKind::Smart {
                    editable: None,
                    source: source.clone(),
                    cache: source.clone(),
                    offset: (0, 0),
                    filters: vec![],
                    placement,
                };
            }
            d.nodes.push(node);
            let bounds = node_bounds(&d, 1).unwrap();
            let pivot = glam::dvec2(
                bounds.x as f64 + bounds.w as f64 / 2.0,
                bounds.y as f64 + bounds.h as f64 / 2.0,
            );
            let rotation = glam::DAffine2::from_translation(pivot)
                * glam::DAffine2::from_angle(37.0_f64.to_radians())
                * glam::DAffine2::from_translation(-pivot);
            Command::RotateNode {
                id: 1,
                degrees: 37.0,
            }
            .apply(&mut d)
            .unwrap();
            let (raster, after) = match &d.nodes[0].kind {
                NodeKind::Raster { raster, placement }
                | NodeKind::Smart {
                    source: raster,
                    placement,
                    ..
                } => (raster, placement),
                _ => panic!(),
            };
            assert!(Arc::ptr_eq(raster, &source));
            for p in [
                glam::dvec2(0.0, 0.0),
                glam::dvec2(17.0, 13.0),
                glam::dvec2(40.0, 20.0),
            ] {
                let want = rotation.transform_point2(placement.to_doc(40, 20).transform_point2(p));
                let actual = after.to_doc(40, 20).transform_point2(p);
                assert!((want - actual).length() < 1e-8);
            }
        }
    }

    #[test]
    fn group_rotation_moves_children_and_masks_about_one_pivot() {
        use emulsion_raster::vector::{Path, PathStyle};
        let mut d = Document::new(120, 120);
        let mut left = Node::raster(
            1,
            "left",
            Arc::new(Raster::solid(10, 10, [1.0, 0.0, 0.0, 1.0])),
            Placement::at(20.0, 40.0),
        );
        left.parent = Some(3);
        let local_mask = Arc::new(emulsion_raster::Mask::empty(10, 10, 255));
        left.mask = Some(local_mask.clone());
        let mut right = Node::path(
            2,
            "path",
            Arc::new(Path::from_svg("M 70 40 L 80 40 L 80 50 L 70 50 Z").unwrap()),
            PathStyle::default(),
            120,
            120,
        );
        right.parent = Some(3);
        let original_path = match &right.kind {
            NodeKind::Path { path, .. } => path.clone(),
            _ => panic!(),
        };
        let mut group = Node::group(3, "pair");
        group.mask = Some(Arc::new(emulsion_raster::select::rect(
            120, 120, 10.0, 30.0, 90.0, 30.0,
        )));
        d.nodes = vec![left, right, group];
        let bounds = node_bounds(&d, 3).unwrap();
        let pivot = glam::dvec2(
            bounds.x as f64 + bounds.w as f64 / 2.0,
            bounds.y as f64 + bounds.h as f64 / 2.0,
        );
        let rotation = glam::DAffine2::from_translation(pivot)
            * glam::DAffine2::from_angle(std::f64::consts::FRAC_PI_2)
            * glam::DAffine2::from_translation(-pivot);
        Command::RotateNode {
            id: 3,
            degrees: 90.0,
        }
        .apply(&mut d)
        .unwrap();
        let NodeKind::Path { path, .. } = &d.node(2).unwrap().kind else {
            panic!()
        };
        let old = original_path.subpaths[0].anchors[0].p;
        let expected = rotation.transform_point2(glam::dvec2(old.0, old.1));
        let new = path.subpaths[0].anchors[0].p;
        assert!((expected - glam::dvec2(new.0, new.1)).length() < 1e-8);
        assert!(Arc::ptr_eq(
            d.node(1).unwrap().mask.as_ref().unwrap(),
            &local_mask
        ));
        let mask_bounds =
            emulsion_raster::select::bounds(d.node(3).unwrap().mask.as_ref().unwrap());
        assert!(
            mask_bounds.h >= 89 && mask_bounds.w <= 31,
            "{mask_bounds:?}"
        );
        assert_eq!(d.node(1).unwrap().parent, Some(3));
        assert_eq!(d.node(2).unwrap().parent, Some(3));
    }

    #[test]
    fn rotate_mask_only_shapes_and_reject_invalid_or_locked_subtrees_atomically() {
        let mut d = Document::new(100, 100);
        let mut fill = Node::new(
            1,
            "shape",
            NodeKind::Fill {
                rgba: [0, 0, 0, 255],
            },
        );
        fill.mask = Some(Arc::new(emulsion_raster::select::rect(
            100, 100, 20.0, 30.0, 40.0, 10.0,
        )));
        d.nodes.push(fill);
        Command::RotateNode {
            id: 1,
            degrees: 90.0,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!(node_bounds(&d, 1), Some(IRect::new(35, 15, 10, 40)));
        for degrees in [f64::NAN, f64::INFINITY] {
            let before = d.nodes.clone();
            assert!(
                Command::RotateNode { id: 1, degrees }
                    .apply(&mut d)
                    .is_err()
            );
            assert_eq!(d.nodes, before);
        }
        d.nodes[0].parent = Some(2);
        d.nodes[0].locked = true;
        d.nodes.push(Node::group(2, "group"));
        let before = d.nodes.clone();
        assert!(matches!(
            Command::RotateNode {
                id: 2,
                degrees: 45.0
            }
            .apply(&mut d),
            Err(crate::CommandError::Locked(1))
        ));
        assert_eq!(d.nodes, before);
        d.nodes[0].locked = false;
        d.nodes[1].locked = true;
        assert!(matches!(
            Command::RotateNode {
                id: 1,
                degrees: 45.0
            }
            .apply(&mut d),
            Err(crate::CommandError::Locked(2))
        ));
    }

    #[test]
    fn crop_moves_layers_without_resampling() {
        let mut d = doc();
        let before = match &d.nodes[0].kind {
            NodeKind::Raster { raster, .. } => raster.clone(),
            _ => unreachable!(),
        };
        Command::Crop {
            rect: IRect::new(90, 20, 40, 40),
            rotation: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!((d.width, d.height), (40, 40));
        let NodeKind::Raster { raster, placement } = &d.nodes[0].kind else {
            panic!()
        };
        assert!(Arc::ptr_eq(raster, &before), "source pixels untouched");
        assert_eq!((placement.x, placement.y), (-90.0, -20.0));
        let flat = flatten(&d.composite_tree(), 0);
        assert!(flat.get(5, 5)[0] > 60000, "left of x=100 is red");
        assert!(flat.get(20, 5)[1] > 60000, "right, top is green");
    }

    #[test]
    fn crop_extends_canvas_and_resize_is_reversible() {
        let mut d = doc();
        Command::Crop {
            rect: IRect::new(-10, -10, 220, 120),
            rotation: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!((d.width, d.height), (220, 120));
        Command::ImageSize {
            width: 110,
            height: 60,
        }
        .apply(&mut d)
        .unwrap();
        Command::ImageSize {
            width: 220,
            height: 120,
        }
        .apply(&mut d)
        .unwrap();
        let NodeKind::Raster { placement, .. } = &d.nodes[0].kind else {
            panic!()
        };
        assert!((placement.x - 10.0).abs() < 1e-9 && (placement.scale_x - 1.0).abs() < 1e-12);
    }

    #[test]
    fn straighten_rotates_placements_about_the_centre() {
        let mut d = doc();
        Command::Crop {
            rect: IRect::new(0, 0, 200, 100),
            rotation: 90.0,
        }
        .apply(&mut d)
        .unwrap();
        let NodeKind::Raster { placement, .. } = &d.nodes[0].kind else {
            panic!()
        };
        assert_eq!(placement.rotation, 90.0);
        assert!(
            (placement.x - 0.0).abs() < 1e-9,
            "centre stays at the canvas centre"
        );
    }

    #[test]
    fn selection_follows_the_crop() {
        let mut d = doc();
        let sel = emulsion_raster::select::rect(200, 100, 100.0, 0.0, 10.0, 10.0);
        Command::SetSelection {
            selection: Some(Arc::new(sel)),
        }
        .apply(&mut d)
        .unwrap();
        Command::Crop {
            rect: IRect::new(95, 0, 20, 20),
            rotation: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        let s = d.selection.clone().unwrap();
        assert_eq!((s.width(), s.height()), (20, 20));
        assert_eq!(s.get(4, 5), 0);
        assert_eq!(s.get(5, 5), 255);
    }
}
