//! The cutter: lift (or copy) the selected part of a layer into a new layer
//! just above it, at the same position. Pixel layers split their pixels by
//! the selection's coverage; vector stroke layers split their strokes and
//! fills along the selection's edge so both layers stay vector.

use crate::command::{Command, Slot};
use crate::document::Document;
use crate::node::{NodeId, NodeKind};
use emulsion_raster::paint::fill_pixels;
use emulsion_raster::stroke_split;
use emulsion_raster::{IRect, Mask, Raster, select};
use glam::dvec2;
use std::sync::Arc;

/// The commands that move (`copy` false) or copy the selected part of
/// layer `id` into a new layer above it. Run them as one Undo step; the
/// new layer is the one `AddNode` returns.
pub fn cut_to_new_layer(
    doc: &Document,
    id: NodeId,
    selection: &Mask,
    copy: bool,
) -> Result<Vec<Command>, String> {
    let node = doc.node(id).ok_or("No such layer")?;
    if select::bounds(selection).is_empty() {
        return Err("Select an area to cut first".into());
    }
    let siblings = doc.children(node.parent);
    let slot = Slot {
        parent: node.parent,
        index: siblings
            .iter()
            .position(|s| *s == id)
            .unwrap_or(siblings.len())
            + 1,
    };
    let mut lifted = node.clone();
    lifted.name = format!("{} {}", node.name, if copy { "copy" } else { "cut" });
    let (kind, rest) = match &node.kind {
        NodeKind::Raster { raster, placement } => {
            let to_doc = placement.to_doc(raster.width(), raster.height());
            let clip = select::local_clip(Arc::new(selection.clone()), to_doc);
            // The selection's bounds in the layer's own pixels.
            let b = select::bounds(selection);
            let inv = to_doc.inverse();
            let corners = [
                (b.x, b.y),
                (b.right(), b.y),
                (b.right(), b.bottom()),
                (b.x, b.bottom()),
            ]
            .map(|(x, y)| inv.transform_point2(dvec2(x as f64, y as f64)));
            let lo = corners
                .iter()
                .fold(dvec2(f64::MAX, f64::MAX), |a, c| a.min(*c));
            let hi = corners
                .iter()
                .fold(dvec2(f64::MIN, f64::MIN), |a, c| a.max(*c));
            let rect = IRect::new(
                lo.x.floor() as i32,
                lo.y.floor() as i32,
                (hi.x.ceil() - lo.x.floor()) as i32 + 1,
                (hi.y.ceil() - lo.y.floor()) as i32 + 1,
            )
            .intersect(&raster.bounds());
            if rect.is_empty() {
                return Err("The selection does not cover this layer".into());
            }
            let px: Vec<[u16; 4]> = raster
                .read_rect(rect)
                .into_iter()
                .enumerate()
                .map(|(i, p)| {
                    let (x, y) = (rect.x + i as i32 % rect.w, rect.y + i as i32 / rect.w);
                    let k = clip(x, y);
                    p.map(|v| (v as f32 * k).round() as u16)
                })
                .collect();
            if px.iter().all(|p| p[3] == 0) {
                return Err("The selection holds no pixels on this layer".into());
            }
            let piece = Raster::transparent(raster.width(), raster.height()).write_rect(rect, &px);
            let rest = (!copy).then(|| {
                let (kept, dirty) = fill_pixels(raster, rect, &|x, y| clip(x, y), &|p, k| {
                    p.map(|v| v * (1.0 - k))
                });
                Command::ReplacePixels {
                    id,
                    raster: Arc::new(kept),
                    dirty,
                    label: "Cut to new layer".into(),
                }
            });
            (
                NodeKind::Raster {
                    raster: Arc::new(piece),
                    placement: *placement,
                },
                rest,
            )
        }
        NodeKind::Strokes { strokes, .. } => {
            let (outside, inside) = stroke_split::split(strokes, selection)?;
            if inside.strokes.is_empty() && inside.fills.is_empty() {
                return Err("The selection holds no strokes on this layer".into());
            }
            let rest = (!copy).then(|| Command::SetStrokes {
                id,
                strokes: Arc::new(outside),
            });
            (
                crate::node::Node::strokes(0, "", Arc::new(inside), doc.width, doc.height).kind,
                rest,
            )
        }
        _ => return Err("The cutter works on pixel and vector stroke layers".into()),
    };
    lifted.kind = kind;
    let mut commands = vec![Command::AddNode {
        node: Box::new(lifted),
        slot,
    }];
    commands.extend(rest);
    Ok(commands)
}
