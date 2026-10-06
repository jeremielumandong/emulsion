//! The paint bucket on pixel and vector stroke layers, as one command:
//! gap closing, fill modes (normal, behind, unpainted) and sampling every
//! visible layer or only the target. On a vector layer the filled area is
//! traced into polygons and added as a [`StrokeFill`], so it stays vector.

use crate::command::Command;
use crate::document::Document;
use crate::node::{NodeId, NodeKind};
use emulsion_raster::composite::region;
use emulsion_raster::gap_fill::{self, FillMode};
use emulsion_raster::select::{self, Combine};
use emulsion_raster::strokes::StrokeFill;
use emulsion_raster::{IRect, Mask, color};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Stair steps up to this size (pixels) straighten when a fill is traced.
const TRACE_TOLERANCE: f64 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BucketOptions {
    /// How far (0–255 per channel) a colour may differ from the clicked one.
    pub tolerance: u8,
    /// Fill only the connected area (otherwise every similar pixel).
    pub contiguous: bool,
    /// Openings in the line art up to this many pixels count as closed.
    pub gap: u32,
    pub mode: FillMode,
    /// Alpha (0–255) below which [`FillMode::Unpainted`] fills a pixel.
    pub threshold: u8,
    /// Find the area on every visible layer (true) or on the target only.
    pub sample_all: bool,
}

impl Default for BucketOptions {
    fn default() -> Self {
        Self {
            tolerance: 32,
            contiguous: true,
            gap: 0,
            mode: FillMode::Normal,
            threshold: 128,
            sample_all: true,
        }
    }
}

/// A document rendered to straight sRGBA8 at full size.
fn srgba8(doc: &Document) -> Result<Vec<u8>, String> {
    let tree = doc.try_composite_tree().map_err(|e| e.to_string())?;
    Ok(region(
        &tree,
        IRect::new(0, 0, tree.width as i32, tree.height as i32),
    )
    .into_iter()
    .flat_map(color::premul_to_srgba8)
    .collect())
}

/// The document-space area a bucket click at `at` fills on layer `id`,
/// within the selection.
pub fn fill_area(
    doc: &Document,
    id: NodeId,
    at: (f64, f64),
    options: &BucketOptions,
) -> Result<Mask, String> {
    let (w, h) = (doc.width, doc.height);
    if !(at.0 >= 0.0 && at.1 >= 0.0 && at.0 < w as f64 && at.1 < h as f64) {
        return Err("The fill point is outside the canvas".into());
    }
    let layer = || {
        let d = doc.solo(id).ok_or("No such layer")?;
        srgba8(&d)
    };
    let image = if options.sample_all {
        srgba8(doc)?
    } else {
        layer()?
    };
    let seed = (at.0 as u32, at.1 as u32);
    let mut area = gap_fill::gap_region(
        &image,
        w,
        h,
        seed,
        options.tolerance,
        options.contiguous,
        options.gap,
    );
    if let Some(s) = &doc.selection {
        area = select::combine(Some(&area), s, Combine::Intersect);
    }
    if options.mode == FillMode::Unpainted
        && matches!(
            doc.node(id).map(|n| &n.kind),
            Some(NodeKind::Strokes { .. })
        )
    {
        // Vector fills cannot test pixels as they paint; drop painted ones.
        let own = if options.sample_all { layer()? } else { image };
        let open = gap_fill::unpainted(&own, w, h, options.threshold);
        area = select::combine(Some(&area), &open, Combine::Intersect);
    }
    Ok(area)
}

/// The command a bucket click at document point `at` makes on layer `id`
/// with straight sRGBA8 `color`, or None when nothing would change.
pub fn bucket_fill(
    doc: &Document,
    id: NodeId,
    at: (f64, f64),
    color: [u8; 4],
    options: &BucketOptions,
) -> Result<Option<Command>, String> {
    let node = doc.node(id).ok_or("No such layer")?;
    if !matches!(
        node.kind,
        NodeKind::Raster { .. } | NodeKind::Strokes { .. }
    ) {
        return Err("The bucket fills pixel and vector stroke layers".into());
    }
    let area = fill_area(doc, id, at, options)?;
    if select::bounds(&area).is_empty() {
        return Ok(None);
    }
    match &node.kind {
        NodeKind::Raster { raster, placement } => {
            let to_doc = placement.to_doc(raster.width(), raster.height());
            let clip = select::local_clip(Arc::new(area), to_doc);
            let (filled, dirty) = gap_fill::fill(
                raster,
                raster.bounds(),
                &|x, y| clip(x, y),
                color::srgba8_to_premul(color),
                options.mode,
                options.threshold,
            );
            Ok(Some(Command::ReplacePixels {
                id,
                raster: Arc::new(filled),
                dirty,
                label: "Fill".into(),
            }))
        }
        NodeKind::Strokes { strokes, .. } => {
            // Reach one pixel under the line art so no seam shows between
            // the fill and antialiased lines drawn over it.
            let reach = select::combine(
                Some(&select::grow(&area, 1)),
                doc.selection
                    .as_deref()
                    .unwrap_or(&select::all(doc.width, doc.height)),
                Combine::Intersect,
            );
            let outlines = gap_fill::trace(&reach, TRACE_TOLERANCE);
            if outlines.is_empty() {
                return Ok(None);
            }
            let fill = StrokeFill { outlines, color };
            let mut set = (**strokes).clone();
            if options.mode == FillMode::Behind {
                set.fills.insert(0, fill);
            } else {
                set.fills.push(fill);
            }
            Ok(Some(Command::SetStrokes {
                id,
                strokes: Arc::new(set),
            }))
        }
        _ => unreachable!("checked above"),
    }
}
