//! Editable native vector-mask geometry. Pixels are always disposable derivations.
use crate::MaskProperties;
use emulsion_raster::{Mask, vector::Path};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[path = "vector_mask_coverage.rs"]
mod coverage;
use coverage::CoveragePath;

/// The infinite coverage of a mask with no anchors. Adding the first anchor
/// uses ordinary nonzero path fill; deleting the last restores this state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmptyVectorCoverage {
    #[default]
    RevealAll,
    HideAll,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorMask {
    pub path: Arc<Path>,
    pub enabled: bool,
    pub linked: bool,
    pub inverted: bool,
    /// Intrinsic path coordinates to layer-local source coordinates.
    pub transform: [f64; 6],
    pub properties: MaskProperties,
    pub empty_coverage: EmptyVectorCoverage,
}
impl Default for VectorMask {
    fn default() -> Self {
        Self::empty(EmptyVectorCoverage::RevealAll)
    }
}
impl VectorMask {
    pub fn empty(empty_coverage: EmptyVectorCoverage) -> Self {
        Self {
            path: Arc::new(Path::default()),
            enabled: true,
            linked: true,
            inverted: false,
            transform: crate::node::default_mask_transform(),
            properties: MaskProperties::default(),
            empty_coverage,
        }
    }
    pub fn valid(&self) -> bool {
        validate_path(&self.path) && self.properties.valid() && valid_transform(self.transform)
    }
    pub fn empty_value(&self) -> u8 {
        let reveal = self.empty_coverage == EmptyVectorCoverage::RevealAll;
        if reveal != self.inverted { 255 } else { 0 }
    }
}

/// Shared with path-layer commands: native editable geometry stays finite and
/// bounded in complexity, without clipping any anchor to the document canvas.
pub fn validate_path(path: &Path) -> bool {
    path.subpaths.len() <= emulsion_raster::vector::MAX_ANCHORS
        && path.anchor_count() <= emulsion_raster::vector::MAX_ANCHORS
        && path
            .subpaths
            .iter()
            .flat_map(|s| &s.anchors)
            .flat_map(|a| [a.p, a.h_in, a.h_out])
            .all(|p| p.0.is_finite() && p.1.is_finite() && p.0.abs() <= 1e9 && p.1.abs() <= 1e9)
}
pub(crate) fn valid_transform(transform: [f64; 6]) -> bool {
    // The intrinsic coordinate limit does not cap affine translation. Large
    // off-canvas placement remains editable provided both directions map the
    // supported coordinate envelope finitely; rendering has its own work gate.
    let m = glam::DAffine2::from_cols_array(&transform);
    transform.iter().all(|v| v.is_finite())
        && m.matrix2.determinant().is_finite()
        && m.matrix2.determinant().abs() >= 1e-12
        && m.inverse().to_cols_array().iter().all(|v| v.is_finite())
        && [-1e9, 1e9].into_iter().all(|x| {
            [-1e9, 1e9].into_iter().all(|y| {
                m.transform_point2(glam::dvec2(x, y)).is_finite()
                    && m.inverse().transform_point2(glam::dvec2(x, y)).is_finite()
            })
        })
}

/// Rasterize only a requested window, never the bounds of the whole path.
/// Off-window edges still participate in the winding rule. The rasterizer
/// clips its scan bounds before integer conversion, so huge off-canvas paths
/// cannot overflow or cause path-bounds-sized allocations.
pub(crate) fn rasterize_path_window(
    path: &Path,
    origin: (f64, f64),
    size: (u32, u32),
) -> Result<Mask, &'static str> {
    let coverage = CoveragePath::new(path, glam::DAffine2::IDENTITY);
    if coverage.work_for_window(origin, size) > MAX_RENDER_WORK {
        return Err(WORK_LIMIT);
    }
    Ok(coverage.rasterize_window(origin, size))
}

/// Per-window memory and cumulative-work limits are independent. The work
/// budget counts raw coverage visits, scanline edge/solve loops, and the actual
/// repeated halo sampling of the shared 64-column feather kernel. Final output
/// writes are separately bounded by the document's 400MP limit.
const MAX_WINDOW_PIXELS: f64 = 16_000_000.0;
const MAX_RENDER_WORK: u64 = 256_000_000;
const MAX_PLAN_WINDOWS: usize = 4096;
const OUTPUT_BLOCK: i32 = 128;
const WORK_LIMIT: &str = "vector mask exceeds the native rendering work budget; reduce feather, path complexity, or extreme mask scale";

#[derive(Clone, Copy)]
enum Window {
    Constant {
        rect: emulsion_raster::IRect,
        value: u8,
    },
    Sharp {
        rect: emulsion_raster::IRect,
    },
    Feather {
        rect: emulsion_raster::IRect,
        origin: glam::DVec2,
        size: (u32, u32),
    },
}
struct Plan {
    coverage: CoveragePath,
    windows: Vec<Window>,
    inverse: glam::DAffine2,
    halo: i32,
    work: u64,
}
impl Plan {
    fn charge(&mut self, work: u64) -> Result<(), &'static str> {
        self.work = self.work.saturating_add(work);
        if self.work > MAX_RENDER_WORK {
            Err(WORK_LIMIT)
        } else {
            Ok(())
        }
    }
    fn push(&mut self, window: Window) -> Result<(), &'static str> {
        if self.windows.len() >= MAX_PLAN_WINDOWS {
            return Err(WORK_LIMIT);
        }
        self.windows.push(window);
        Ok(())
    }
}
fn split(
    rect: emulsion_raster::IRect,
    horizontal: bool,
    pending: &mut Vec<emulsion_raster::IRect>,
) {
    use emulsion_raster::IRect;
    if horizontal {
        let n = rect.w / 2;
        pending.push(IRect::new(rect.x + n, rect.y, rect.w - n, rect.h));
        pending.push(IRect::new(rect.x, rect.y, n, rect.h));
    } else {
        let n = rect.h / 2;
        pending.push(IRect::new(rect.x, rect.y + n, rect.w, rect.h - n));
        pending.push(IRect::new(rect.x, rect.y, rect.w, n));
    }
}
fn plan(mask: &VectorMask, size: (u32, u32), offset: (i32, i32)) -> Result<Plan, &'static str> {
    use emulsion_raster::IRect;
    use glam::{DAffine2, DVec2, dvec2};
    if size.0 == 0
        || size.1 == 0
        || size.0 > crate::document::MAX_SIDE
        || size.1 > crate::document::MAX_SIDE
        || u64::from(size.0) * u64::from(size.1) > crate::document::MAX_PIXELS
    {
        return Err("vector mask output grid exceeds native dimensions");
    }
    let affine = crate::composite_mask_cache::mask_to_output(mask.transform, offset);
    let feather = mask.properties.feather >= 0.5;
    let coverage = CoveragePath::new(
        &mask.path,
        if feather { DAffine2::IDENTITY } else { affine },
    );
    let inverse = affine.inverse();
    let halo = if feather {
        3 * (mask.properties.feather / 1.7).round().max(1.0) as i32
    } else {
        0
    };
    let mut plan = Plan {
        coverage,
        windows: Vec::new(),
        inverse,
        halo,
        work: 0,
    };
    let mut pending = vec![IRect::new(0, 0, size.0 as i32, size.1 as i32)];
    while let Some(rect) = pending.pop() {
        plan.charge(1)?;
        if !feather {
            let low = dvec2(f64::from(rect.x), f64::from(rect.y));
            let high = dvec2(f64::from(rect.right()), f64::from(rect.bottom()));
            let (constant, work) = plan.coverage.constant_rect(low, high);
            plan.charge(work)?;
            if let Some(value) = constant {
                plan.push(Window::Constant { rect, value })?;
                continue;
            }
            if rect.w > OUTPUT_BLOCK || rect.h > OUTPUT_BLOCK {
                split(rect, rect.w >= rect.h, &mut pending);
                continue;
            }
            plan.charge(
                plan.coverage
                    .work_for_window((low.x, low.y), (rect.w as u32, rect.h as u32)),
            )?;
            plan.push(Window::Sharp { rect })?;
            continue;
        }
        let point = |x: i32, y: i32| {
            inverse.transform_point2(dvec2(f64::from(x) + 0.5, f64::from(y) + 0.5))
        };
        let corners = [
            point(rect.x, rect.y),
            point(rect.right() - 1, rect.y),
            point(rect.x, rect.bottom() - 1),
            point(rect.right() - 1, rect.bottom() - 1),
        ];
        if corners.iter().any(|point| !point.is_finite()) {
            return Err("vector mask output sampling is non-finite");
        }
        let mut lo = corners[0];
        let mut hi = corners[0];
        for p in corners {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        let origin = (lo - DVec2::splat(0.5)).floor();
        let end = (hi - DVec2::splat(0.5)).floor() + DVec2::splat(2.0);
        let extent = end - origin;
        let low = origin - DVec2::splat(f64::from(halo));
        let high = end + DVec2::splat(f64::from(halo));
        let (constant, work) = plan.coverage.constant_rect(low, high);
        plan.charge(work)?;
        if let Some(value) = constant {
            plan.push(Window::Constant { rect, value })?;
            continue;
        }
        let padded = high - low;
        if !padded.is_finite() || padded.x * padded.y > MAX_WINDOW_PIXELS {
            if rect.w == 1 && rect.h == 1 {
                return Err(WORK_LIMIT);
            }
            let xs = inverse.matrix2.x_axis.length() * f64::from(rect.w - 1);
            let ys = inverse.matrix2.y_axis.length() * f64::from(rect.h - 1);
            split(rect, rect.w > 1 && (rect.h == 1 || xs >= ys), &mut pending);
            continue;
        }
        let (w, h) = (extent.x as u32, extent.y as u32);
        if w == 0 || h == 0 {
            return Err(WORK_LIMIT);
        }
        let raw_size = (w + 2 * halo as u32, h + 2 * halo as u32);
        plan.charge(plan.coverage.work_for_window((low.x, low.y), raw_size))?;
        let rows = u64::from(raw_size.1);
        let strips = u64::from(w).div_ceil(64);
        let horizontal = (u64::from(w) + 2 * halo as u64 * strips).saturating_mul(rows);
        let vertical = u64::from(w).saturating_mul(rows);
        plan.charge(
            horizontal
                .saturating_mul(8)
                .saturating_add(vertical.saturating_mul(10)),
        )?;
        plan.push(Window::Feather {
            rect,
            origin,
            size: (w, h),
        })?;
    }
    Ok(plan)
}

/// Commands and every decoded live/history document run this preflight before
/// publishing a snapshot. Rejection is explicit and atomic, never a substituted
/// reveal/hide result or a runtime mask disappearance.
pub(crate) fn validate_render(
    mask: &VectorMask,
    size: (u32, u32),
    offset: (i32, i32),
) -> Result<(), &'static str> {
    if mask.path.is_empty() || mask.properties.density == 0.0 {
        return Ok(());
    }
    plan(mask, size, offset).map(|_| ())
}

/// Accepted plans preserve the intrinsic kernel exactly. Constant support
/// rectangles skip rasterization/blur by proof, including extreme minification.
pub(crate) fn render(mask: &VectorMask, size: (u32, u32), offset: (i32, i32)) -> Mask {
    use emulsion_raster::IRect;
    use glam::{DVec2, dvec2};
    if mask.path.is_empty() || mask.properties.density == 0.0 {
        let value = if mask.properties.density == 0.0 {
            255
        } else {
            density(mask.empty_value(), mask.properties.density)
        };
        return Mask::empty(size.0, size.1, value);
    }
    let fill = density(if mask.inverted { 255 } else { 0 }, mask.properties.density);
    let mut out = Mask::empty(size.0, size.1, fill);
    let plan = plan(mask, size, offset).expect("vector mask render requires a validated document");
    let point = |x: i32, y: i32| {
        plan.inverse
            .transform_point2(dvec2(f64::from(x) + 0.5, f64::from(y) + 0.5))
    };
    for window in &plan.windows {
        match *window {
            Window::Constant { rect, value } => {
                let value = density(
                    if mask.inverted { 255 - value } else { value },
                    mask.properties.density,
                );
                if value == fill {
                    continue;
                }
                // Even an enormous constant support needs only bounded output
                // chunks; never allocate a full final-plane temporary Vec.
                for y in (rect.y..rect.bottom()).step_by(OUTPUT_BLOCK as usize) {
                    for x in (rect.x..rect.right()).step_by(OUTPUT_BLOCK as usize) {
                        let region = IRect::new(
                            x,
                            y,
                            (rect.right() - x).min(OUTPUT_BLOCK),
                            (rect.bottom() - y).min(OUTPUT_BLOCK),
                        );
                        out = out.write_rect(
                            region,
                            &vec![value; region.w as usize * region.h as usize],
                        );
                    }
                }
            }
            Window::Sharp { rect } => {
                let raw = plan.coverage.rasterize_window(
                    (f64::from(rect.x), f64::from(rect.y)),
                    (rect.w as u32, rect.h as u32),
                );
                let values = raw
                    .to_gray8()
                    .into_iter()
                    .map(|v| {
                        density(
                            if mask.inverted { 255 - v } else { v },
                            mask.properties.density,
                        )
                    })
                    .collect::<Vec<_>>();
                out = out.write_rect(rect, &values);
            }
            Window::Feather {
                rect,
                origin,
                size: (w, h),
            } => {
                let halo = plan.halo;
                let low = origin - DVec2::splat(f64::from(halo));
                let raw = plan
                    .coverage
                    .rasterize_window((low.x, low.y), (w + 2 * halo as u32, h + 2 * halo as u32));
                let feathered = emulsion_raster::select::feather_sampled_region(
                    w,
                    h,
                    if mask.inverted { 255 } else { 0 },
                    IRect::new(0, 0, w as i32, h as i32),
                    mask.properties.feather,
                    |x, y| {
                        let v = raw.get((x + halo) as u32, (y + halo) as u32);
                        if mask.inverted { 255 - v } else { v }
                    },
                );
                let processed = MaskProperties {
                    density: mask.properties.density,
                    feather: 0.0,
                }
                .apply(&feathered);
                // Final output writes are block bounded even when one useful
                // intrinsic window covers a large destination rectangle.
                for y in (rect.y..rect.bottom()).step_by(OUTPUT_BLOCK as usize) {
                    for x in (rect.x..rect.right()).step_by(OUTPUT_BLOCK as usize) {
                        let region = IRect::new(
                            x,
                            y,
                            (rect.right() - x).min(OUTPUT_BLOCK),
                            (rect.bottom() - y).min(OUTPUT_BLOCK),
                        );
                        let values = (region.y..region.bottom())
                            .flat_map(|y| {
                                let processed = &processed;
                                (region.x..region.right()).map(move |x| {
                                    crate::transform::sample_mask(processed, point(x, y) - origin)
                                })
                            })
                            .collect::<Vec<_>>();
                        out = out.write_rect(region, &values);
                    }
                }
            }
        }
    }
    out
}
fn density(value: u8, density: f32) -> u8 {
    (255.0 - density * (255.0 - f32::from(value))).round() as u8
}

#[cfg(test)]
mod bounded_work_tests {
    use super::*;
    use crate::{Command, Document, Editor, Node};
    use emulsion_raster::{Placement, Raster, vector_geometry};
    fn scene(mask: VectorMask) -> Document {
        let mut doc = Document::new(32, 24);
        let mut node = Node::raster(
            1,
            "Vector work budget",
            Arc::new(Raster::solid(32, 24, [1.; 4])),
            Placement::default(),
        );
        node.vector_mask = Some(mask);
        doc.nodes.push(node);
        doc.next_id = 2;
        doc
    }
    fn stripes() -> VectorMask {
        let mut path = Path::default();
        for x in 0..32 {
            path.subpaths.extend(
                vector_geometry::rectangle(x as f64 * 100_000., 0., 50_000., 2_400_000.).subpaths,
            );
        }
        VectorMask {
            path: Arc::new(path),
            transform: [1e-5, 0., 0., 1e-5, 0., 0.],
            ..Default::default()
        }
    }
    #[test]
    fn nonempty_extreme_minification_with_max_feather_proves_constant_support() {
        for inverted in [false, true] {
            for density_value in [0.25, 1.0] {
                let mask = VectorMask {
                    path: Arc::new(vector_geometry::rectangle(0., 0., 3_200_000., 2_400_000.)),
                    transform: [1e-5, 0., 0., 1e-5, 0., 0.],
                    inverted,
                    properties: MaskProperties {
                        density: density_value,
                        feather: 1000.,
                    },
                    ..Default::default()
                };
                let path = mask.path.clone();
                let plan = plan(&mask, (32, 24), (0, 0)).unwrap();
                assert!(plan.work < 64, "constant proof cost {}", plan.work);
                assert!(matches!(
                    plan.windows.as_slice(),
                    [Window::Constant { value: 255, .. }]
                ));
                let doc = scene(mask.clone());
                doc.validate().unwrap();
                let expected = density(if inverted { 0 } else { 255 }, density_value);
                assert!(
                    doc.vector_mask_for_inspection(&doc.nodes[0])
                        .unwrap()
                        .unwrap()
                        .to_gray8()
                        .into_iter()
                        .all(|v| v == expected)
                );
                assert!(Arc::ptr_eq(
                    &path,
                    &doc.nodes[0].vector_mask.as_ref().unwrap().path
                ));
            }
        }
    }
    #[test]
    fn cumulative_feather_work_rejects_before_mutation_and_preserves_history() {
        for enabled in [false, true] {
            let mut mask = stripes();
            mask.enabled = enabled;
            let before = scene(mask);
            before.validate().unwrap();
            let mut editor = Editor::new(before.clone(), None);
            let error = editor
                .execute(Command::SetVectorMaskProperties {
                    id: 1,
                    properties: MaskProperties {
                        density: 1.,
                        feather: 1000.,
                    },
                })
                .unwrap_err()
                .to_string();
            assert!(error.contains("rendering work budget"), "{error}");
            assert_eq!(editor.doc, before);
            assert!(editor.history.is_empty());
            assert!(!editor.undo());
            let mut invalid = before.clone();
            invalid.nodes[0]
                .vector_mask
                .as_mut()
                .unwrap()
                .properties
                .feather = 1000.;
            assert!(
                invalid
                    .validate()
                    .unwrap_err()
                    .to_string()
                    .contains("rendering work budget")
            );
            invalid.nodes[0]
                .vector_mask
                .as_mut()
                .unwrap()
                .properties
                .density = 0.;
            invalid.validate().unwrap(); // Explicitly dormant coverage needs no work.
        }
    }
    #[test]
    fn full_extent_vector_rasterization_rejects_excessive_work_atomically() {
        let mut path = Path::default();
        for _ in 0..128 {
            path.subpaths
                .extend(vector_geometry::rectangle(0., 0., 4000., 4000.).subpaths);
        }
        let before = scene(VectorMask {
            path: Arc::new(path),
            ..Default::default()
        });
        before.validate().unwrap();
        let mut editor = Editor::new(before.clone(), None);
        let error = editor
            .execute(Command::RasterizeVectorMask { id: 1 })
            .unwrap_err()
            .to_string();
        assert!(error.contains("rendering work budget"), "{error}");
        assert_eq!(editor.doc, before);
        assert!(editor.history.is_empty());
    }
}
