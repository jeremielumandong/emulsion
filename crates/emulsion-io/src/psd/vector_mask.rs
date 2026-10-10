//! Bounded editable PSD vector masks. ag-psd 0.3 deliberately uses unit document
//! dimensions for paths, so its knot coordinates are normalized, not pixels.
//!
//! The PSD specification documents even-odd path fill. Native masks use nonzero winding. Only
//! one provably simple contour (or an empty descriptor) crosses that boundary;
//! an operation flag alone is not evidence that compound fills are equivalent.

use ag_psd::psd::{
    BezierKnot, BezierPath, BooleanOperation, FillRule, Layer, LayerMaskData, LayerVectorMask,
};
use emulsion_core::{Document, EmptyVectorCoverage, MaskProperties, Node, NodeKind, VectorMask};
use emulsion_raster::vector::{Anchor, Path, SubPath};
use std::sync::Arc;

#[path = "simple_contour.rs"]
mod simple_contour;

const PATH_SCALE: f64 = (1u32 << 24) as f64;

fn properties(layer: &Layer) -> Option<MaskProperties> {
    let parameters = layer.additional_info.mask.as_ref();
    let density = parameters
        .and_then(|m| m.vector_mask_density)
        .unwrap_or(1.0);
    let feather = parameters
        .and_then(|m| m.vector_mask_feather)
        .unwrap_or(0.0);
    if !density.is_finite()
        || !(0.0..=1.0).contains(&density)
        || !feather.is_finite()
        || !(0.0..=f64::from(emulsion_core::MAX_MASK_FEATHER)).contains(&feather)
    {
        return None;
    }
    let result = MaskProperties {
        density: density as f32,
        feather: feather as f32,
    };
    result.valid().then_some(result)
}

/// The PSD format reserves three guard bits in signed 8.24 path coordinates. Refuse
/// out-of-range points rather than letting the dependency's i32 cast saturate.
fn normalized_coordinate(value: f64) -> bool {
    value.is_finite() && (-16.0..16.0).contains(&value)
}

fn supported_paths(mask: &LayerVectorMask) -> bool {
    if mask.clipboard.is_some() || mask.paths.len() > 1 {
        return false;
    }
    let Some(path) = mask.paths.first() else {
        return true;
    };
    if path.operation != Some(BooleanOperation::Combine)
        || path.knots.len() < 2
        || path.knots.len() > emulsion_raster::vector::MAX_ANCHORS
        || path.knots.iter().any(|k| {
            k.points.len() != 6
                || !k
                    .points
                    .iter()
                    .all(|v| normalized_coordinate(*v) && (v * PATH_SCALE).fract() == 0.0)
        })
    {
        return false;
    }
    // The raw guard rejects unknown fill fields. Both recognized rules agree
    // on this subset, including implicit straight closure of open contours.
    simple_contour::is_simple(path)
}

pub(super) fn can_import(layer: &Layer) -> bool {
    layer
        .additional_info
        .vector_mask
        .as_ref()
        .is_none_or(|mask| supported_paths(mask) && properties(layer).is_some())
}

pub(super) fn import(layer: &Layer, size: (u32, u32), origin: (f64, f64)) -> Option<VectorMask> {
    let mask = layer.additional_info.vector_mask.as_ref()?;
    if !supported_paths(mask) {
        return None;
    }
    let point = |x: f64, y: f64| (x * f64::from(size.0), y * f64::from(size.1));
    let path = Path {
        subpaths: mask
            .paths
            .iter()
            .map(|path| SubPath {
                closed: !path.open,
                anchors: path
                    .knots
                    .iter()
                    .map(|k| Anchor {
                        h_in: point(k.points[0], k.points[1]),
                        p: point(k.points[2], k.points[3]),
                        h_out: point(k.points[4], k.points[5]),
                        smooth: k.linked,
                    })
                    .collect(),
            })
            .collect(),
    };
    Some(VectorMask {
        path: Arc::new(path),
        enabled: !mask.disable.unwrap_or(false),
        linked: !mask.not_link.unwrap_or(false),
        inverted: mask.invert.unwrap_or(false),
        // Keep the PSD's document-space geometry, then map it to source pixels.
        transform: [1.0, 0.0, 0.0, 1.0, -origin.0, -origin.1],
        properties: properties(layer)?,
        empty_coverage: if mask.fill_starts_with_all_pixels.unwrap_or(false) {
            EmptyVectorCoverage::RevealAll
        } else {
            EmptyVectorCoverage::HideAll
        },
    })
}

fn origin(node: &Node) -> Option<(f64, f64)> {
    match &node.kind {
        NodeKind::Group { .. } => Some((0.0, 0.0)),
        NodeKind::Raster { placement, .. } if super::raster_placement_is_translated(placement) => {
            Some((placement.x, placement.y))
        }
        NodeKind::Smart { placement, .. } if super::smart_objects::can_export(node) => {
            let placement = placement.legacy()?;
            Some((placement.x, placement.y))
        }
        _ => None,
    }
}

/// Nonzero properties need the standard mask-parameter header. ag-psd emits a
/// synthetic empty -2 channel if it has a header without a raster bitmap, and
/// misreads parameter blocks >=36 bytes as real-mask records. Neither case is
/// an editable round-trip: disclose the existing appearance fallback instead.
fn parameters_representable(node: &Node, mask: &VectorMask, at: (f64, f64)) -> bool {
    let p = mask.properties;
    if p == MaskProperties::default() {
        return true;
    }
    if node.mask.is_none() {
        return false;
    }
    let raster = if super::editable_mask_origin(node, at.0, at.1).is_some() {
        node.mask_properties
    } else {
        MaskProperties::default()
    };
    let size = 18
        + 1
        + 2
        + usize::from(raster.density != 1.0)
        + 8 * usize::from(raster.feather != 0.0)
        + usize::from(p.density != 1.0)
        + 8 * usize::from(p.feather != 0.0);
    size < 36
}

pub(super) fn export(doc: &Document, node: &Node) -> Option<LayerVectorMask> {
    let mask = node.vector_mask.as_ref()?;
    let at = origin(node)?;
    if mask.path.subpaths.len() > 1 || !parameters_representable(node, mask, at) {
        return None;
    }
    let [a, b, c, d, tx, ty] = mask.transform;
    // Native feather is applied in intrinsic pixels. Only translation commutes
    // with the PSD document-space feather parameter used by this bounded route.
    if mask.properties.feather != 0.0
        && ([a, b, c, d] != [1.0, 0.0, 0.0, 1.0] || tx.fract() != 0.0 || ty.fract() != 0.0)
    {
        return None;
    }
    let point = |p: (f64, f64)| -> Option<[f64; 2]> {
        let x = (a * p.0 + c * p.1 + tx + at.0) / f64::from(doc.width);
        let y = (b * p.0 + d * p.1 + ty + at.1) / f64::from(doc.height);
        if !normalized_coordinate(x) || !normalized_coordinate(y) {
            return None;
        }
        // Quantize deliberately and test the actual emitted geometry. The
        // dependency writes truncating 8.24; pre-rounded grid values are exact.
        let x = (x * PATH_SCALE).round() / PATH_SCALE;
        let y = (y * PATH_SCALE).round() / PATH_SCALE;
        (normalized_coordinate(x) && normalized_coordinate(y)).then_some([x, y])
    };
    let paths = mask
        .path
        .subpaths
        .iter()
        .map(|path| {
            let knots = path
                .anchors
                .iter()
                .map(|anchor| {
                    let i = point(anchor.h_in)?;
                    let p = point(anchor.p)?;
                    let o = point(anchor.h_out)?;
                    Some(BezierKnot {
                        linked: anchor.smooth,
                        points: vec![i[0], i[1], p[0], p[1], o[0], o[1]],
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(BezierPath {
                open: !path.closed,
                operation: Some(BooleanOperation::Combine),
                fill_rule: FillRule::EvenOdd,
                knots,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let result = LayerVectorMask {
        invert: Some(mask.inverted),
        not_link: Some(!mask.linked),
        disable: Some(!mask.enabled),
        fill_starts_with_all_pixels: Some(mask.empty_coverage == EmptyVectorCoverage::RevealAll),
        paths,
        ..Default::default()
    };
    supported_paths(&result).then_some(result)
}

pub(super) fn add_parameters(node: &Node, raster: &mut Option<LayerMaskData>) {
    if let (Some(vector), Some(raster)) = (&node.vector_mask, raster) {
        if vector.properties.density != 1.0 {
            raster.vector_mask_density = Some(f64::from(vector.properties.density));
        }
        if vector.properties.feather != 0.0 {
            raster.vector_mask_feather = Some(f64::from(vector.properties.feather));
        }
    }
}
