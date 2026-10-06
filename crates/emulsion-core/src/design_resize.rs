//! Named, physical resize targets and read-only checks of a prepared variant.
//! The resize engine keeps native nodes; warnings never modify/flatten artwork.
use crate::creation::{CanvasSpec, Unit};
use crate::{Command, Document, Editor, Node, NodeId, NodeKind};
use emulsion_raster::vector_geometry;
use glam::dvec2;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct ResizePreset {
    pub key: &'static str,
    pub name: &'static str,
    pub width: f64,
    pub height: f64,
    pub unit: Unit,
    pub resolution: f32,
}
impl ResizePreset {
    pub fn pixel_size(&self) -> Result<(u32, u32), String> {
        CanvasSpec {
            width: self.width,
            height: self.height,
            unit: self.unit,
            resolution: f64::from(self.resolution),
            ..Default::default()
        }
        .pixel_size()
    }
    /// Authored units and resolution, plus rounded output pixels for print.
    pub fn dimensions_label(&self) -> String {
        let size = format!("{} × {} {}", self.width, self.height, self.unit.label());
        match (self.unit, self.pixel_size()) {
            (Unit::Pixels, _) => format!("{size} · {} ppi", self.resolution),
            (_, Ok((w, h))) => format!("{size} · {} ppi · {w} × {h} px", self.resolution),
            _ => format!("{size} · {} ppi", self.resolution),
        }
    }
}
pub const PRESETS: &[ResizePreset] = &[
    ResizePreset {
        key: "square_post",
        name: "Square post",
        width: 1080.,
        height: 1080.,
        unit: Unit::Pixels,
        resolution: 72.,
    },
    ResizePreset {
        key: "portrait_post",
        name: "Portrait post",
        width: 1080.,
        height: 1350.,
        unit: Unit::Pixels,
        resolution: 72.,
    },
    ResizePreset {
        key: "story",
        name: "Story",
        width: 1080.,
        height: 1920.,
        unit: Unit::Pixels,
        resolution: 72.,
    },
    ResizePreset {
        key: "slide",
        name: "Wide slide",
        width: 1920.,
        height: 1080.,
        unit: Unit::Pixels,
        resolution: 72.,
    },
    ResizePreset {
        key: "a4",
        name: "A4 print",
        width: 210.,
        height: 297.,
        unit: Unit::Millimeters,
        resolution: 300.,
    },
    ResizePreset {
        key: "letter",
        name: "US Letter print",
        width: 8.5,
        height: 11.,
        unit: Unit::Inches,
        resolution: 300.,
    },
];

pub struct ResizePlan {
    pub doc: Document,
    /// Visible artwork extending beyond the canvas, excluding intentional image
    /// cropping inside a native frame and role-pinned page backgrounds.
    pub overflow: Vec<NodeId>,
    /// Editable text whose shaped line/column breaks changed.
    pub text_reflow: Vec<NodeId>,
    /// Text extending beyond its paragraph frame before renderer clipping.
    pub text_overflow: Vec<NodeId>,
    /// Framed raster images whose geometric extent leaves a frame edge exposed.
    /// This does not guarantee opaque pixel coverage or inspect raster alpha.
    pub photo_coverage: Vec<NodeId>,
    /// Path/warp text or masked/effected/clipped content needs a visual check.
    pub unchecked: Vec<NodeId>,
    pub background_recropped: bool,
}

/// Build from the immutable original on every preview, never from a previous
/// preview. Physical units convert through CanvasSpec, using the chosen ppi.
pub fn prepare(
    source: &Document,
    width: u32,
    height: u32,
    resolution: f32,
) -> Result<ResizePlan, String> {
    CanvasSpec {
        width: f64::from(width),
        height: f64::from(height),
        resolution: f64::from(resolution),
        ..Default::default()
    }
    .pixel_size()?;
    let mut resized = crate::design_metadata::resize_variant(source, width, height)?;
    resized.doc.resolution = resolution;
    crate::design_layout::reflow_after(source, &mut resized.doc)?;
    validate_resize_locks(source, &resized.doc)?;
    resized.doc.validate().map_err(|error| error.to_string())?;
    Ok(analyze(source, resized.doc))
}

fn visible(doc: &Document, id: NodeId) -> bool {
    let mut at = Some(id);
    while let Some(node) = at.and_then(|id| doc.node(id)) {
        if !node.visible || node.opacity == 0. {
            return false;
        }
        at = node.parent;
    }
    true
}
fn clipping_check(doc: &Document, id: NodeId) -> (bool, bool) {
    let mut at = Some(id);
    let mut clipped = false;
    let mut outside = false;
    let bounds = crate::geometry::affine_capability_bounds(doc, id);
    while let Some(node) = at.and_then(|id| doc.node(id)) {
        if let Some(base) = node.clip_to {
            clipped = true;
            if let (Some(content), Some(frame)) =
                (bounds, crate::geometry::affine_capability_bounds(doc, base))
            {
                outside |= content.x < frame.x
                    || content.y < frame.y
                    || content.right() > frame.right()
                    || content.bottom() > frame.bottom();
            }
        }
        at = node.parent;
    }
    (clipped, outside)
}
fn analyze(source: &Document, doc: Document) -> ResizePlan {
    let mut plan = ResizePlan {
        doc,
        overflow: Vec::new(),
        text_reflow: Vec::new(),
        text_overflow: Vec::new(),
        photo_coverage: Vec::new(),
        unchecked: Vec::new(),
        background_recropped: false,
    };
    for node in &plan.doc.nodes {
        if !visible(&plan.doc, node.id) {
            continue;
        }
        if node.has_mask() || !node.styles.is_empty() {
            plan.unchecked.push(node.id);
        }
        if let NodeKind::Text { spec, .. } = &node.kind {
            let (clipped, outside) = clipping_check(&plan.doc, node.id);
            if clipped {
                plan.unchecked.push(node.id);
            }
            if outside {
                plan.text_overflow.push(node.id);
            }
            match crate::text::resize_flow(spec) {
                Some((lines, overflow)) => {
                    if overflow {
                        plan.text_overflow.push(node.id);
                    }
                    if let Some(NodeKind::Text { spec: original, .. }) =
                        source.node(node.id).map(|node| &node.kind)
                        && let Some((before, _)) = crate::text::resize_flow(original)
                        && lines != before
                    {
                        plan.text_reflow.push(node.id);
                    }
                }
                None => plan.unchecked.push(node.id),
            }
        }
        if let NodeKind::Raster { placement, .. } = &node.kind
            && node.clip_to.is_some()
        {
            // Fit intent is not stored on imported/arbitrary groups. Do not
            // claim a simple Cover check validates their authored composition.
            let simple = node.parent.is_some_and(|parent| {
                plan.doc.node(parent).is_some_and(|group| !group.has_mask())
                    && plan.doc.children(Some(parent)).iter().all(|id| {
                        *id == node.id
                            || matches!(
                                plan.doc.node(*id).map(|n| &n.kind),
                                Some(NodeKind::Path { .. })
                            )
                    })
            });
            if !simple || (placement.scale_x - placement.scale_y).abs() > 1e-9 {
                plan.unchecked.push(node.id);
            }
            match image_covers_frame(&plan.doc, node.id) {
                Some(false) => plan.photo_coverage.push(node.id),
                None => plan.unchecked.push(node.id),
                _ => {}
            }
        }
        if crate::design_background::is_background_node(&plan.doc, node.id)
            || node.is_group()
            || matches!(node.kind, NodeKind::Fill { .. } | NodeKind::Adjust(_))
        {
            continue;
        }
        // Native clip boundaries are tested independently, so ordinary Cover
        // overhang does not masquerade as a page overflow warning.
        if node.clip_to.is_some() {
            continue;
        }
        if crate::geometry::affine_capability_bounds(&plan.doc, node.id).is_some_and(|bounds| {
            bounds.x < 0
                || bounds.y < 0
                || bounds.right() > plan.doc.width as i32
                || bounds.bottom() > plan.doc.height as i32
        }) {
            plan.overflow.push(node.id);
        }
    }
    plan.unchecked.sort_unstable();
    plan.unchecked.dedup();
    plan.text_overflow.sort_unstable();
    plan.text_overflow.dedup();
    plan.background_recropped = crate::design_background::parts(source)
        .and_then(|background| background.image)
        .is_some_and(|image| source.node(image.image) != plan.doc.node(image.image));
    plan
}

/// Compare the complete vector frame bounds in source-image coordinates. This
/// includes rotated/flipped images without confusing their axis-aligned bbox
/// with actual coverage. Raster alpha and effects are deliberately not assessed.
pub(crate) fn image_covers_frame(doc: &Document, image: NodeId) -> Option<bool> {
    let node = doc.node(image)?;
    let NodeKind::Raster { raster, placement } = &node.kind else {
        return None;
    };
    let NodeKind::Path { path, .. } = &doc.node(node.clip_to?)?.kind else {
        return None;
    };
    let mut local = (**path).clone();
    local.transform(placement.to_doc(raster.width(), raster.height()).inverse());
    let (x, y, w, h) = vector_geometry::bounds(&local)?;
    Some(
        x >= -0.01
            && y >= -0.01
            && x + w <= f64::from(raster.width()) + 0.01
            && y + h <= f64::from(raster.height()) + 0.01,
    )
}

fn native_cover_parts(source: &Document, group: NodeId) -> Option<(NodeId, NodeId, Vec<NodeId>)> {
    source
        .node(group)
        .filter(|node| node.is_group() && !node.has_mask())?;
    let (boundary, Some(image)) = crate::design::frame_parts(source, group)? else {
        return None;
    };
    if image_covers_frame(source, image) != Some(true)
        || !matches!(&source.node(image)?.kind,NodeKind::Raster {placement,..} if (placement.scale_x-placement.scale_y).abs()<=1e-9)
    {
        return None;
    }
    let members = source.children(Some(group));
    if members.iter().any(|id| {
        *id != image
            && !matches!(
                source.node(*id).map(|node| &node.kind),
                Some(NodeKind::Path { .. })
            )
    }) {
        return None;
    }
    Some((boundary, image, members))
}
/// Reuse native frame fitting within ordinary maskless groups. Each frame's
/// boundary transforms once, while its photo retains an undistorted placement.
/// Responsive/masked/ambiguous frame groups use the explicit fallback guard.
pub(crate) fn resize_cover_frame(
    source: &Document,
    editor: &mut Editor,
    group: NodeId,
    transform: [f64; 6],
) -> Result<bool, String> {
    let Some(node) = source.node(group).filter(|node| node.is_group()) else {
        return Ok(false);
    };
    let mut ancestor = node.parent;
    while let Some(id) = ancestor {
        if source.design.frames.contains_key(&id) {
            return Ok(false);
        }
        ancestor = source.node(id).and_then(|node| node.parent);
    }
    if let Some((boundary, image, members)) = native_cover_parts(source, group) {
        let (focus, zoom) = crop_parameters(source, boundary, image)?;
        let ids = members.into_iter().filter(|id| *id != image).collect();
        editor
            .execute(Command::TransformNodes { ids, transform })
            .map_err(|error| error.to_string())?;
        refit_cover(editor, image, focus, zoom)?;
        return Ok(true);
    }
    let descendants = source.subtree(group);
    let photos: Vec<_> = descendants
        .iter()
        .filter_map(|id| source.node(*id))
        .filter(|node| matches!(node.kind, NodeKind::Raster { .. }) && node.clip_to.is_some())
        .collect();
    if photos.is_empty()
        || descendants.iter().any(|id| {
            source.design.frames.contains_key(id)
                || source
                    .node(*id)
                    .is_some_and(|node| node.is_group() && node.has_mask())
        })
        || photos.iter().any(|photo| {
            photo
                .parent
                .and_then(|parent| native_cover_parts(source, parent))
                .is_none()
        })
    {
        return Ok(false);
    }
    // No responsive descendants or ancestors can reflow partially transformed
    // geometry, and group geometry is entirely expressed by its native children.
    for child in source.children(Some(group)) {
        if !resize_cover_frame(source, editor, child, transform)? {
            editor
                .execute(Command::TransformNodes {
                    ids: vec![child],
                    transform,
                })
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(true)
}
/// The native Placement model cannot represent an arbitrary skew. Complex
/// grouped photos must not silently stretch merely because their rotation is
/// zero; reject unsupported aspect changes consistently and preserve the source.
pub(crate) fn check_grouped_photo_transform(
    source: &Document,
    id: NodeId,
    sx: f64,
    sy: f64,
) -> Result<(), String> {
    if (sx - sy).abs() <= 1e-9 {
        return Ok(());
    }
    if let Some(photo) = source
        .subtree(id)
        .into_iter()
        .filter_map(|id| source.node(id))
        .find(|node| matches!(node.kind, NodeKind::Raster { .. }) && node.clip_to.is_some())
    {
        return Err(format!(
            "“{}” needs an individual frame resize before this grouped or non-Cover photo can change aspect ratio. Use proportional dimensions or resize the frame separately.",
            photo.name
        ));
    }
    Ok(())
}

fn crop_parameters(
    source: &Document,
    boundary: NodeId,
    image: NodeId,
) -> Result<([f64; 2], f64), String> {
    let NodeKind::Raster { raster, placement } =
        &source.node(image).ok_or("Missing frame image")?.kind
    else {
        return Err("Invalid frame image".into());
    };
    let NodeKind::Path { path, .. } = &source.node(boundary).ok_or("Missing frame boundary")?.kind
    else {
        return Err("Invalid frame boundary".into());
    };
    let (x, y, w, h) = vector_geometry::bounds(path).ok_or("Empty frame boundary")?;
    let centre = placement
        .to_doc(raster.width(), raster.height())
        .inverse()
        .transform_point2(dvec2(x + w / 2., y + h / 2.));
    let focus = [
        (centre.x / f64::from(raster.width())).clamp(0., 1.),
        (centre.y / f64::from(raster.height())).clamp(0., 1.),
    ];
    let Command::SetPlacement {
        placement: base, ..
    } = crate::design::fit_frame_image(source, image, crate::design::ImageFit::Cover, focus)?
    else {
        unreachable!()
    };
    let zoom = (placement.scale_x / base.scale_x)
        .min(placement.scale_y / base.scale_y)
        .max(1.);
    Ok((focus, zoom))
}
fn refit_cover(
    editor: &mut Editor,
    image: NodeId,
    focus: [f64; 2],
    zoom: f64,
) -> Result<(), String> {
    let fit =
        crate::design::fit_frame_image(&editor.doc, image, crate::design::ImageFit::Cover, focus)?;
    editor.execute(fit).map_err(|error| error.to_string())?;
    if zoom > 1. + 1e-9 {
        let crop = crate::design::crop_frame_image(&editor.doc, image, [0.; 2], zoom)?;
        editor.execute(crop).map_err(|error| error.to_string())?;
    }
    // Cover fitting clamps before zoom. Restore the authored focal point after
    // zoom too, when the extra crop space can make an off-centre focus possible.
    let (boundary, _) =
        crate::design::frame_parts(&editor.doc, image).ok_or("Missing image frame")?;
    let NodeKind::Path { path, .. } = &editor.doc.node(boundary).unwrap().kind else {
        unreachable!()
    };
    let (x, y, w, h) = vector_geometry::bounds(path).ok_or("Empty image frame")?;
    let NodeKind::Raster { raster, placement } = &editor.doc.node(image).unwrap().kind else {
        unreachable!()
    };
    let point = placement
        .to_doc(raster.width(), raster.height())
        .transform_point2(dvec2(
            focus[0] * f64::from(raster.width()),
            focus[1] * f64::from(raster.height()),
        ));
    let pan = [x + w / 2. - point.x, y + h / 2. - point.y];
    let crop = crate::design::crop_frame_image(&editor.doc, image, pan, 1.)?;
    editor.execute(crop).map_err(|error| error.to_string())?;
    Ok(())
}

/// A pinned background follows the canvas despite its manual-edit locks. Crop
/// around its existing focal point and keep its relative user-authored zoom.
pub(crate) fn resize_background(source: &Document, editor: &mut Editor) -> Result<(), String> {
    let Some(image) =
        crate::design_background::parts(source).and_then(|background| background.image)
    else {
        return Ok(());
    };
    if (source.width, source.height) == (editor.doc.width, editor.doc.height) {
        return Ok(());
    }
    // Only role-owned locks are bypassed; unrelated foreground stays protected.
    let mut unlocked = source.clone();
    for node in &mut unlocked.nodes {
        if [image.group, image.boundary, image.image].contains(&node.id) {
            node.locked = false;
            node.locks = Default::default();
        }
    }
    let (focus, zoom) = crop_parameters(&unlocked, image.boundary, image.image)?;
    let NodeKind::Path { style, .. } = &editor
        .doc
        .node(image.boundary)
        .ok_or("Missing background boundary")?
        .kind
    else {
        return Err("Invalid background boundary".into());
    };
    let style = *style;
    editor
        .execute(Command::SetPath {
            id: image.boundary,
            path: Arc::new(vector_geometry::rectangle(
                0.,
                0.,
                f64::from(editor.doc.width),
                f64::from(editor.doc.height),
            )),
            style,
        })
        .map_err(|error| error.to_string())?;
    refit_cover(editor, image.image, focus, zoom)?;
    Ok(())
}

fn background_geometry_only(before: &Node, after: &Node) -> bool {
    let kind_allowed = match (&before.kind, &after.kind) {
        (NodeKind::Path { style: a, .. }, NodeKind::Path { style: b, .. }) => a == b,
        (NodeKind::Raster { raster: a, .. }, NodeKind::Raster { raster: b, .. }) => {
            Arc::ptr_eq(a, b)
        }
        (a, b) => a == b,
    };
    let mut same = after.clone();
    same.kind = before.kind.clone();
    kind_allowed && &same == before
}
/// Used by both preparation and the final project commit. It is intentionally
/// narrow: only the same role nodes may change geometry under a background lock.
pub(crate) fn validate_resize_locks(source: &Document, next: &Document) -> Result<(), String> {
    for before in &source.nodes {
        let locks = source.layer_locks(before.id);
        if source.locked_ancestor(before.id).is_none() && locks == Default::default() {
            continue;
        }
        if next.node(before.id) == Some(before) {
            continue;
        }
        if source.design.page_background == next.design.page_background
            && crate::design_background::is_background_node(source, before.id)
            && next
                .node(before.id)
                .is_some_and(|after| background_geometry_only(before, after))
        {
            continue;
        }
        return Err(format!(
            "Unlock “{}” before resizing this page.",
            before.name
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "design_resize_analysis_tests.rs"]
mod tests;
