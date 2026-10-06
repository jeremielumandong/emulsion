//! Export-only page expansion. The editable source document is never changed.
use crate::{IoError, Result};
use emulsion_core::{Document, Node, NodeKind, vector_cache::VectorRaster};
use emulsion_raster::{BlendMode, IRect, vector::PathPaint, vector_geometry};
use glam::dvec2;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackgroundPhotoWarning {
    /// The placed source image does not geometrically reach every bleed corner.
    SourceTooSmall,
    /// A customized clipping boundary was retained rather than changing artwork.
    CustomizedFrame,
}

fn dimensions(doc: &Document, bleed_mm: f64) -> Result<(u32, u32, u32)> {
    if !bleed_mm.is_finite() || bleed_mm < 0. {
        return Err(IoError::Manifest(
            "Bleed must be a finite, non-negative number of millimeters.".into(),
        ));
    }
    if !doc.resolution.is_finite() || doc.resolution <= 0. {
        return Err(IoError::Manifest(
            "Page resolution must be finite and greater than zero.".into(),
        ));
    }
    let pixels = (bleed_mm * f64::from(doc.resolution) / 25.4).round();
    // Bound floating-point input before integer conversion/addition. Rust's
    // saturating float cast would otherwise silently accept NaN/huge values.
    if !pixels.is_finite() || pixels > f64::from(emulsion_core::document::MAX_SIDE) {
        return Err(IoError::Manifest(
            "The requested bleed is too large.".into(),
        ));
    }
    let bleed = pixels as u32;
    let width = doc.width.checked_add(bleed * 2);
    let height = doc.height.checked_add(bleed * 2);
    let (Some(width), Some(height)) = (width, height) else {
        return Err(IoError::Manifest(
            "The requested bleed is too large.".into(),
        ));
    };
    crate::import::check_size(width, height)?;
    Ok((width, height, bleed))
}

/// Only the native, invisible page-sized clip may grow automatically. Replacing
/// a user-edited shape, painted border, gradient or masked clip could alter trim
/// artwork. Leave those intact and report their coverage as unverified instead.
fn standard_boundary(node: &Node, width: u32, height: u32) -> bool {
    let NodeKind::Path { path, style, .. } = &node.kind else {
        return false;
    };
    node.opacity == 0.
        && node.blend == BlendMode::Normal
        && node.blending == Default::default()
        && !node.has_mask()
        && node.styles.is_empty()
        && node.clip_to.is_none()
        && style.fill.is_some_and(|color| color[3] == 255)
        && style.fill_paint == PathPaint::Solid
        && style.stroke.is_none()
        && **path == vector_geometry::rectangle(0., 0., f64::from(width), f64::from(height))
}

pub(super) fn with_bleed(doc: &Document, bleed_mm: f64) -> Result<(Document, u32)> {
    let (width, height, bleed) = dimensions(doc, bleed_mm)?;
    let boundary = doc
        .design
        .page_background
        .and_then(|background| background.image)
        .and_then(|frame| doc.node(frame.boundary))
        .filter(|node| standard_boundary(node, doc.width, doc.height))
        .map(|node| node.id);
    let mut doc = crate::export::develop_document(doc)?;
    if bleed > 0 {
        emulsion_core::geometry::crop(
            &mut doc,
            IRect::new(
                -(bleed as i32),
                -(bleed as i32),
                width as i32,
                height as i32,
            ),
            0.,
        )?;
        if let Some(boundary) = boundary {
            let NodeKind::Path { path, style, cache } = &mut doc.node_mut(boundary).unwrap().kind
            else {
                unreachable!("Developing and translating preserve native path nodes")
            };
            *path = Arc::new(vector_geometry::rectangle(
                0.,
                0.,
                f64::from(width),
                f64::from(height),
            ));
            *cache = VectorRaster::path(path.clone(), *style, width, height);
        }
    }
    // Export-only crop changes document-space vector sampling windows.
    doc.validate()?;
    Ok((doc, bleed))
}

/// Geometric preflight for a document returned by `with_bleed`. Source pixels,
/// rotation, flips, alpha, opacity and authored masks are never changed. `None`
/// is not a guarantee of opaque photo coverage: transparency/masks may
/// intentionally expose the Fill even when the source rectangle covers the page.
///
/// If the source ends at trim, exposing genuine off-page pixels cannot fill the
/// bleed. Report that limit instead of silently zooming/re-cropping or inventing
/// repeated edge pixels. An explicitly hidden photo requires no warning.
pub(super) fn background_photo_warning(
    doc: &Document,
    bleed: u32,
) -> Option<BackgroundPhotoWarning> {
    warning_in_bounds(
        doc,
        bleed,
        [0., 0., f64::from(doc.width), f64::from(doc.height)],
    )
}

/// Cheap layout/print preflight in the original document's coordinates. This
/// reads native geometry only: it never clones or transforms the document,
/// develops a linked RAW original, renders a mask, or samples source pixels.
pub(super) fn original_background_photo_warning(
    doc: &Document,
    bleed_mm: f64,
) -> Result<Option<BackgroundPhotoWarning>> {
    let (_, _, bleed) = dimensions(doc, bleed_mm)?;
    let margin = f64::from(bleed);
    Ok(warning_in_bounds(
        doc,
        bleed,
        [
            -margin,
            -margin,
            f64::from(doc.width) + margin,
            f64::from(doc.height) + margin,
        ],
    ))
}

fn warning_in_bounds(
    doc: &Document,
    bleed: u32,
    [left, top, right, bottom]: [f64; 4],
) -> Option<BackgroundPhotoWarning> {
    if bleed == 0 {
        return None;
    }
    let frame = doc.design.page_background?.image?;
    let group = doc.node(frame.group)?;
    let boundary = doc.node(frame.boundary)?;
    let image = doc.node(frame.image)?;
    if !group.visible
        || group.opacity == 0.
        || !boundary.visible
        || !image.visible
        || image.opacity == 0.
    {
        return None;
    }
    if !standard_boundary(boundary, doc.width, doc.height) {
        return Some(BackgroundPhotoWarning::CustomizedFrame);
    }
    let NodeKind::Raster { raster, placement } = &image.kind else {
        return Some(BackgroundPhotoWarning::CustomizedFrame);
    };
    // An axis-aligned bounding box is insufficient for a rotated photograph:
    // inverse-transform all bleed corners into the original source rectangle.
    let inverse = placement.to_doc(raster.width(), raster.height()).inverse();
    let source_size = dvec2(f64::from(raster.width()), f64::from(raster.height()));
    let covered = [
        dvec2(left, top),
        dvec2(right, top),
        dvec2(left, bottom),
        dvec2(right, bottom),
    ]
    .into_iter()
    .all(|point| {
        let p = inverse.transform_point2(point);
        p.is_finite() && p.min_element() >= -1e-7 && (p - source_size).max_element() <= 1e-7
    });
    (!covered).then_some(BackgroundPhotoWarning::SourceTooSmall)
}

#[cfg(test)]
#[path = "project_export_bleed_tests.rs"]
mod tests;
