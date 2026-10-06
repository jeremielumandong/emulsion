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
    dimensions_from_node(doc.node(id)?)
}
pub(crate) fn dimensions_from_node(node: &crate::Node) -> Option<(u32, u32)> {
    match &node.kind {
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
    let old = editor.doc.node(id).expect("existing source");
    let mut candidate = old.clone();
    match &mut candidate.kind {
        NodeKind::Raster { raster, placement } => {
            *raster = source;
            placement.scale_x *= f64::from(w) / f64::from(nw);
            placement.scale_y *= f64::from(h) / f64::from(nh);
        }
        NodeKind::Smart {
            source: pixels,
            editable,
            original_image,
            placement,
            ..
        } => {
            *pixels = source;
            *editable = None;
            *original_image = None;
            *placement = match *placement {
                crate::SmartPlacement::Legacy(mut p) => {
                    p.scale_x *= f64::from(w) / f64::from(nw);
                    p.scale_y *= f64::from(h) / f64::from(nh);
                    crate::SmartPlacement::Legacy(p)
                }
                crate::SmartPlacement::Projective(map) => {
                    let scale = emulsion_raster::projective::Projective2::from_affine(
                        glam::DAffine2::from_scale(glam::dvec2(
                            f64::from(w) / f64::from(nw),
                            f64::from(h) / f64::from(nh),
                        )),
                    )
                    .map_err(|e| e.to_string())?;
                    crate::SmartPlacement::Projective(
                        map.compose(scale).map_err(|e| e.to_string())?,
                    )
                }
            };
        }
        _ => unreachable!(),
    }
    // All components, including latent and unlinked descriptors, keep their
    // old world basis. Certify the prospective footprint before filter work.
    crate::transform::preserve_components(old, &mut candidate, true).map_err(|e| e.to_string())?;
    if matches!(candidate.kind, NodeKind::Smart { .. }) {
        crate::smart_support::preflight_stack_support(
            crate::smart_support::metadata_for_node(&candidate).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let NodeKind::Smart {
            source,
            filters,
            filter_styles,
            filters_enabled,
            cache,
            offset,
            ..
        } = &mut candidate.kind
        else {
            unreachable!()
        };
        (*cache, *offset) =
            crate::smart::render_stack(source, filters, filter_styles, *filters_enabled);
        crate::smart_support::validate_node(&candidate).map_err(|e| e.to_string())?;
    }
    let mut next = editor.doc.clone();
    *next.node_mut(id).expect("existing source") = candidate;
    next.validate().map_err(|e| e.to_string())?;
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
    node.require_affine_capability("source crop")
        .map_err(|e| e.to_string())?;
    if !node.mask_properties.is_default() {
        return Err(
            "Apply or reset mask density and feather before cropping the source image.".into(),
        );
    }
    if node.mask_transform != crate::Mapping2::IDENTITY || !node.mask_enabled {
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
        let placement = placement.require_legacy("legacy fixture").unwrap();
        assert_eq!(placement.scale_x, 0.5);
        assert_eq!(placement.x, 10.);
        e.undo();
        assert_eq!(e.doc, before);
        let revision = e.revision;
        assert!(crop(&mut e, id, [0., 0., 20., 20.]).is_err());
        assert_eq!(e.revision, revision);
    }
}
