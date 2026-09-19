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
            let p = crate::smart::cache_placement(
                placement,
                (source.width(), source.height()),
                (cache.width(), cache.height()),
                *offset,
            );
            return Some(mapped_bounds(
                ink_bounds(cache, mask),
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
