//! Native image replacement and non-destructive source-pixel crop masks.
use crate::{Command, Document, Editor, NodeId, NodeKind};
use emulsion_raster::{Mask, Raster};
use std::sync::Arc;
fn editable(doc: &Document, id: NodeId) -> Result<(), String> {
    let locks = doc.layer_locks(id);
    if doc.locked_ancestor(id).is_some() || locks.pixels || locks.position || locks.transparency {
        return Err("Unlock the image before changing its source.".into());
    }
    Ok(())
}
pub fn dimensions(doc: &Document, id: NodeId) -> Option<(u32, u32)> {
    match &doc.node(id)?.kind {
        NodeKind::Raster { raster, .. } => Some((raster.width(), raster.height())),
        NodeKind::Smart { source, .. } => Some((source.width(), source.height())),
        _ => None,
    }
}
/// Keeps the displayed rectangle, rotation, effects and filter stack. Native
/// editable smart sources become the explicitly chosen raster source.
pub fn replace(editor: &mut Editor, id: NodeId, source: Arc<Raster>) -> Result<(), String> {
    editable(&editor.doc, id)?;
    let (w, h) = dimensions(&editor.doc, id).ok_or("Select an image or Smart Object.")?;
    let (nw, nh) = (source.width(), source.height());
    if nw == 0
        || nh == 0
        || nw > crate::document::MAX_SIDE
        || nh > crate::document::MAX_SIDE
        || u64::from(nw) * u64::from(nh) > crate::document::MAX_PIXELS
    {
        return Err("Replacement image dimensions exceed the document limits.".into());
    }
    let mut next = editor.doc.clone();
    let node = next.node_mut(id).unwrap();
    if let Some(mask) = &node.mask
        && (w, h) != (nw, nh)
    {
        if node.mask_transform != crate::node::default_mask_transform() {
            return Err("Reset the transformed layer mask before replacing with a differently sized source.".into());
        }
        let mut data = Vec::with_capacity(nw as usize * nh as usize);
        for y in 0..nh {
            for x in 0..nw {
                data.push(mask.get(
                    (u64::from(x) * u64::from(w) / u64::from(nw)) as u32,
                    (u64::from(y) * u64::from(h) / u64::from(nh)) as u32,
                ));
            }
        }
        node.mask = Some(Arc::new(Mask::from_gray8(nw, nh, &data)));
    }
    match &mut node.kind {
        NodeKind::Raster { raster, placement } => {
            *raster = source;
            placement.scale_x *= f64::from(w) / f64::from(nw);
            placement.scale_y *= f64::from(h) / f64::from(nh);
        }
        NodeKind::Smart {
            source: old,
            editable,
            filters,
            filter_styles,
            placement,
            cache,
            offset,
        } => {
            let (rendered, origin) = crate::smart::render_styled(&source, filters, filter_styles);
            *old = source;
            *editable = None;
            *cache = rendered;
            *offset = origin;
            placement.scale_x *= f64::from(w) / f64::from(nw);
            placement.scale_y *= f64::from(h) / f64::from(nh);
        }
        _ => unreachable!(),
    }
    crate::design_component_inference::infer(&editor.doc, &mut next);
    editor.commit_design_document(next, "Replace image source")
}
/// Intersects the existing untransformed mask; source pixels stay intact.
pub fn crop(editor: &mut Editor, id: NodeId, rect: [f64; 4]) -> Result<(), String> {
    editable(&editor.doc, id)?;
    let (w, h) = dimensions(&editor.doc, id).ok_or("Select an image or Smart Object.")?;
    let [x, y, width, height] = rect;
    if rect.iter().any(|v| !v.is_finite())
        || x < 0.
        || y < 0.
        || width <= 0.
        || height <= 0.
        || x + width > f64::from(w)
        || y + height > f64::from(h)
    {
        return Err("Crop bounds must fit inside the source image in pixels.".into());
    }
    let node = editor.doc.node(id).unwrap();
    if node.mask_transform != crate::node::default_mask_transform() || !node.mask_enabled {
        return Err("Enable and reset the layer mask transform before cropping.".into());
    }
    let mut data = vec![0; w as usize * h as usize];
    for py in y.floor() as u32..((y + height).ceil() as u32).min(h) {
        for px in x.floor() as u32..((x + width).ceil() as u32).min(w) {
            let coverage = ((f64::from(px + 1).min(x + width) - f64::from(px).max(x)).max(0.)
                * (f64::from(py + 1).min(y + height) - f64::from(py).max(y)).max(0.))
                as f32;
            let old = node.mask.as_ref().map_or(255, |m| m.get(px, py));
            data[py as usize * w as usize + px as usize] =
                (f32::from(old) * coverage).round() as u8;
        }
    }
    editor
        .execute(Command::SetMask {
            id,
            mask: Some(Arc::new(Mask::from_gray8(w, h, &data))),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, command::Slot};
    use emulsion_raster::Placement;
    #[test]
    fn image_source_replace_crop_preserves_filters_pixels_and_undo() {
        let mut e = Editor::new(Document::new(100, 100), None);
        let source = Arc::new(Raster::from_srgba8(10, 10, &vec![255; 400]));
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Image",
                    source.clone(),
                    Placement::at(10., 20.),
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        e.execute(Command::ConvertToSmart { id }).unwrap();
        let before = e.doc.clone();
        crop(&mut e, id, [1., 2., 5., 6.]).unwrap();
        assert_eq!(e.doc.node(id).unwrap().mask.as_ref().unwrap().get(0, 0), 0);
        let NodeKind::Smart {
            source: current, ..
        } = &e.doc.node(id).unwrap().kind
        else {
            panic!()
        };
        assert!(Arc::ptr_eq(current, &source));
        e.undo();
        assert_eq!(e.doc, before);
        replace(
            &mut e,
            id,
            Arc::new(Raster::from_srgba8(20, 20, &vec![128; 1600])),
        )
        .unwrap();
        let NodeKind::Smart { placement, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(placement.scale_x, 0.5);
        assert_eq!(placement.x, 10.);
        e.undo();
        assert_eq!(e.doc, before);
        let revision = e.revision;
        assert!(crop(&mut e, id, [0., 0., 20., 20.]).is_err());
        assert_eq!(e.revision, revision);
    }
}
