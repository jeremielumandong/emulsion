//! Baking a layer mask into content is a single undoable command.
use crate::{CommandError, Document, NodeKind};
use emulsion_raster::{Placement, Raster, color};
use std::sync::Arc;

pub(crate) fn apply(doc: &mut Document, id: u64) -> Result<Option<u64>, CommandError> {
    let node = doc.node(id).ok_or(CommandError::NoSuchNode(id))?;
    node.require_affine_capability("ApplyLayerMask")?;
    if node.mask.is_none() {
        return Err(CommandError::NoSuchParam(id, "layer mask".into()));
    }
    // Apply only the enabled raster component. The vector component remains
    // editable and is still evaluated once by the compositor afterward.
    let mask = if node.mask_enabled {
        doc.raster_mask_for_inspection(node)?
    } else {
        None
    };
    let vector_world = crate::transform::vector_mask_to_document(node)?;
    let vector_transform = match (&node.kind, &node.vector_mask) {
        (NodeKind::Smart { .. }, Some(mask)) => Some(
            crate::composite_mask_cache::mask_to_output(
                mask.transform,
                crate::smart_support::output_grid(node)?.offset,
            )
            .to_cols_array(),
        ),
        _ => None,
    };
    let (source, placement) = match &node.kind {
        NodeKind::Raster { raster, placement } => (raster.clone(), *placement),
        NodeKind::Text { cache, .. }
        | NodeKind::Path { cache, .. }
        | NodeKind::Strokes { cache, .. } => (cache.pixels().clone(), Placement::default()),
        NodeKind::Smart {
            source, placement, ..
        } => {
            let grid = crate::smart_support::output_grid(node)?;
            (
                crate::smart_filter_mask::effective_pixels_with_space(node, doc.blend_space)?
                    .expect("Smart node"),
                crate::smart::cache_placement(
                    &placement.require_legacy("ApplyLayerMask")?,
                    (source.width(), source.height()),
                    grid.size,
                    grid.offset,
                ),
            )
        }
        NodeKind::Fill { rgba } => (
            Arc::new(Raster::empty(
                doc.width,
                doc.height,
                color::f_to_px(color::srgba8_to_premul(*rgba)),
            )),
            Placement::default(),
        ),
        _ => {
            return Err(CommandError::NoSuchParam(
                id,
                "apply mask requires pixel, text, path, fill, or Smart content".into(),
            ));
        }
    };
    let raster = if let Some(mask) = mask {
        Arc::new(Raster::from_fn(
            source.width(),
            source.height(),
            [0; 4],
            |x, y| {
                let coverage = u32::from(mask.get(x, y));
                source
                    .get(x, y)
                    .map(|v| ((u32::from(v) * coverage + 127) / 255) as u16)
            },
        ))
    } else {
        source
    };
    let node = doc.node_mut(id).unwrap();
    node.kind = NodeKind::Raster { raster, placement };
    if let (Some(mask), Some(transform)) = (&mut node.vector_mask, vector_transform) {
        mask.transform = transform;
    } else {
        crate::transform::preserve_vector_mask_world(node, vector_world)?;
    }
    node.mask = None;
    node.mask_properties = Default::default();
    node.mask_enabled = true;
    node.mask_transform = crate::Mapping2::IDENTITY;
    node.mask_linked = true;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Node, command::Slot, history::Editor};
    use emulsion_raster::Mask;
    #[test]
    fn applying_mask_preserves_visible_pixels_and_undo_restores_source() {
        let mut editor = Editor::new(Document::new(4, 2), None);
        let source = Arc::new(Raster::solid(4, 2, [1., 0., 0., 1.]));
        let mask = Arc::new(Mask::from_fn(4, 2, 0, |x, _| if x < 2 { 128 } else { 0 }));
        let mut node = Node::raster(0, "Masked", source.clone(), Placement::default());
        node.mask = Some(mask.clone());
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before =
            emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0).to_srgba8();
        editor.execute(Command::ApplyLayerMask { id }).unwrap();
        assert!(editor.doc.node(id).unwrap().mask.is_none());
        assert_eq!(
            emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0).to_srgba8(),
            before
        );
        assert!(editor.undo());
        let node = editor.doc.node(id).unwrap();
        assert!(Arc::ptr_eq(node.mask.as_ref().unwrap(), &mask));
        let NodeKind::Raster { raster, .. } = &node.kind else {
            panic!()
        };
        assert!(Arc::ptr_eq(raster, &source));
    }

    #[test]
    fn apply_disabled_mask_preserves_appearance_and_advanced_blending() {
        for enabled in [false, true] {
            let mut doc = Document::new(4, 2);
            let mut node = Node::raster(
                1,
                "Source",
                Arc::new(Raster::solid(4, 2, [0.8, 0.2, 0.1, 1.])),
                Placement::default(),
            );
            node.mask = Some(Arc::new(Mask::empty(4, 2, 128)));
            node.mask_enabled = enabled;
            node.blend = emulsion_raster::BlendMode::Screen;
            node.opacity = 0.6;
            node.blending.fill_opacity = 0.4;
            node.blending.channels[2] = false;
            node.blending.blend_if.source.black_fade = 0.1;
            doc.nodes.push(Node::new(
                2,
                "Background",
                NodeKind::Fill {
                    rgba: [30, 60, 90, 255],
                },
            ));
            doc.nodes.push(node.clone());
            doc.next_id = 3;
            let before = emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8();
            Command::ApplyLayerMask { id: 1 }.apply(&mut doc).unwrap();
            assert_eq!(doc.node(1).unwrap().blending, node.blending);
            assert_eq!(doc.node(1).unwrap().blend, node.blend);
            assert_eq!(doc.node(1).unwrap().opacity, node.opacity);
            assert_eq!(
                emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8(),
                before
            );
        }
    }
}
