//! Explicit, bounded conversion of one editable vector component to raster data.
use crate::{CommandError, Document, DocumentError, NodeId, VectorMask};
use emulsion_raster::Mask;
use glam::{DAffine2, dvec2};
use std::sync::Arc;

// Conversion needs the complete intrinsic path, unlike disposable viewport
// rendering. Bound that deliberate allocation rather than discarding geometry.
const MAX_RASTERIZED_MASK_PIXELS: u64 = 16 * 1024 * 1024;

/// Store intrinsic coverage and retain the affine and live properties. This
/// fixes the path's resolution at intrinsic pixels; transformed unfeathered
/// antialiasing may differ from fresh vector rendering, as with rasterization
/// of any editable geometry. No density/feather is baked or applied twice.
fn rasterize_intrinsic(
    mask: &VectorMask,
    id: NodeId,
) -> Result<(Arc<Mask>, [f64; 6]), CommandError> {
    if !mask.valid() {
        return Err(DocumentError::BadValue(id, "vector mask").into());
    }
    if mask.path.is_empty() {
        return Ok((
            Arc::new(Mask::empty(1, 1, mask.empty_value())),
            mask.transform,
        ));
    }
    // The control-point hull contains every cubic and implicit fill-closing
    // edge. Keep the entire hull, including negative/off-canvas coordinates.
    let mut low = dvec2(f64::INFINITY, f64::INFINITY);
    let mut high = dvec2(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for point in mask
        .path
        .subpaths
        .iter()
        .flat_map(|s| &s.anchors)
        .flat_map(|anchor| [anchor.p, anchor.h_in, anchor.h_out])
    {
        let point = dvec2(point.0, point.1);
        low = low.min(point);
        high = high.max(point);
    }
    // Whole-pixel padding retains antialias samples on the hull boundary. The
    // raster-mask processor independently retains the complete feather halo.
    let origin = low.floor() - dvec2(1.0, 1.0);
    let extent = high.ceil() + dvec2(1.0, 1.0) - origin;
    if !extent.is_finite()
        || extent.x > f64::from(crate::document::MAX_SIDE)
        || extent.y > f64::from(crate::document::MAX_SIDE)
        || extent.x * extent.y > MAX_RASTERIZED_MASK_PIXELS as f64
    {
        return Err(CommandError::NoSuchParam(
            id,
            "vector mask is too large to rasterize without clipping its geometry".into(),
        ));
    }
    let size = (extent.x.max(1.0) as u32, extent.y.max(1.0) as u32);
    let raw = crate::vector_mask::rasterize_path_window(&mask.path, (origin.x, origin.y), size)
        .map_err(|message| CommandError::NoSuchParam(id, message.into()))?;
    let raw = if mask.inverted {
        Mask::from_fn(size.0, size.1, 255, |x, y| 255 - raw.get(x, y))
    } else {
        raw
    };
    let transform = DAffine2::from_cols_array(&mask.transform) * DAffine2::from_translation(origin);
    if !crate::vector_mask::valid_transform(transform.to_cols_array()) {
        return Err(DocumentError::BadValue(id, "vector mask transform").into());
    }
    Ok((Arc::new(raw), transform.to_cols_array()))
}

/// Convert only when the raster slot is empty. Independent components cannot
/// be collapsed while preserving their enabled/link state and editability.
/// All validation and allocation completes before the node changes.
pub(crate) fn rasterize(doc: &mut Document, id: NodeId) -> Result<Option<NodeId>, CommandError> {
    let node = doc.node(id).ok_or(CommandError::NoSuchNode(id))?;
    node.require_affine_capability("rasterize vector mask")?;
    if let Some(locked) = doc.locked_ancestor(id) {
        return Err(CommandError::Locked(locked));
    }
    if doc.layer_locks(id).position {
        return Err(CommandError::Locked(id));
    }
    let vector = node
        .vector_mask
        .as_ref()
        .ok_or_else(|| CommandError::NoSuchParam(id, "vector mask".into()))?;
    if node.mask.is_some() {
        return Err(CommandError::NoSuchParam(
            id,
            "remove or apply the raster mask before rasterizing the vector mask".into(),
        ));
    }
    let (raw, transform) = rasterize_intrinsic(vector, id)?;
    let (enabled, linked, properties) = (vector.enabled, vector.linked, vector.properties);
    let node = doc.node_mut(id).unwrap();
    node.mask = Some(raw);
    node.mask_transform = crate::Mapping2::Affine(DAffine2::from_cols_array(&transform));
    node.mask_enabled = enabled;
    node.mask_linked = linked;
    node.mask_properties = properties;
    node.vector_mask = None;
    Ok(None)
}
