//! Editable geometry effects for text layers.
//!
//! Effects contain only model data and deterministic geometry. Font shaping
//! remains in [`crate::text`], which applies these mappings after shaping so
//! the original text and its typography remain editable.

use emulsion_raster::vector::{Path, Pt};
use serde::{Deserialize, Serialize};

/// Photoshop-style editable warp presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarpStyle {
    #[default]
    None,
    Arc,
    Bulge,
    Flag,
}

/// A non-destructive text warp. Values are percentages in `-100..=100`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextWarp {
    pub style: WarpStyle,
    pub bend: f32,
    pub horizontal: f32,
    pub vertical: f32,
}

impl TextWarp {
    pub fn sanitized(mut self) -> Self {
        self.bend = percent(self.bend);
        self.horizontal = percent(self.horizontal);
        self.vertical = percent(self.vertical);
        self
    }

    pub fn is_identity(self) -> bool {
        let w = self.sanitized();
        (w.style == WarpStyle::None || w.bend.abs() < f32::EPSILON)
            && w.horizontal.abs() < f32::EPSILON
            && w.vertical.abs() < f32::EPSILON
    }
}

/// How editable text uses its attached vector path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextPathMode {
    #[default]
    Follow,
    Inside,
}

/// A local-coordinate path attached to an editable text layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextPath {
    pub path: Path,
    pub mode: TextPathMode,
    /// Distance along a followed path, in local pixels.
    pub offset: f32,
    /// Put followed text on the opposite side of the path.
    pub flip: bool,
    /// Padding from the boundary for text placed inside a closed path.
    pub inset: f32,
}

impl Default for TextPath {
    fn default() -> Self {
        Self {
            path: Path::default(),
            mode: TextPathMode::Follow,
            offset: 0.0,
            flip: false,
            inset: 0.0,
        }
    }
}

impl TextPath {
    pub fn sanitized(mut self) -> Self {
        self.offset = finite(self.offset, 0.0).clamp(-1_000_000.0, 1_000_000.0);
        self.inset = finite(self.inset, 0.0).clamp(0.0, 10_000.0);
        // Rendering already bounds path complexity, but text must also be safe
        // when a hand-authored document reaches the core directly.
        if self.path.anchor_count() > emulsion_raster::vector::MAX_ANCHORS {
            self.path = Path::default();
        }
        self
    }

    /// Local bounds used to size paragraph text placed inside a path.
    pub fn inner_bounds(&self) -> Option<EffectRect> {
        let mut bounds = path_bounds(&self.path)?;
        let inset = finite(self.inset, 0.0).clamp(0.0, 10_000.0);
        bounds.x += inset;
        bounds.y += inset;
        bounds.width -= inset * 2.0;
        bounds.height -= inset * 2.0;
        (bounds.width >= 1.0 && bounds.height >= 1.0).then_some(bounds)
    }

    pub fn contains(&self, point: Pt) -> bool {
        if self.mode != TextPathMode::Inside {
            return true;
        }
        let Some(bounds) = self.inner_bounds() else {
            return false;
        };
        if point.0 < bounds.x as f64
            || point.1 < bounds.y as f64
            || point.0 > (bounds.x + bounds.width) as f64
            || point.1 > (bounds.y + bounds.height) as f64
        {
            return false;
        }
        let inside = winding_contains(&self.path, point);
        inside && (self.inset <= 0.0 || distance_to_path(&self.path, point) >= self.inset as f64)
    }
}

/// Axis-aligned local geometry used by the effect mapping API.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EffectRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl EffectRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }
}

/// Map a shaped local point through warp and optional path geometry.
///
/// `baseline` is the first shaped baseline in local coordinates. Along-path
/// text uses the point's distance from it as the normal offset.
pub fn map_point(
    point: Pt,
    shaped_bounds: EffectRect,
    baseline: f32,
    warp: TextWarp,
    text_path: Option<&TextPath>,
) -> Pt {
    TextEffectMapper::new(shaped_bounds, baseline, warp, text_path).map(point)
}

/// Prepared effect geometry for mapping many glyph samples without repeatedly
/// flattening the attached vector path.
pub struct TextEffectMapper {
    bounds: EffectRect,
    baseline: f32,
    warp: TextWarp,
    path: Option<PreparedTextPath>,
}

enum PreparedTextPath {
    Follow {
        measure: PathMeasure,
        offset: f64,
        flip: bool,
    },
    Inside {
        inner: EffectRect,
        contours: Vec<Vec<Pt>>,
        inset: f64,
    },
}

impl TextEffectMapper {
    pub fn new(
        bounds: EffectRect,
        baseline: f32,
        warp: TextWarp,
        text_path: Option<&TextPath>,
    ) -> Self {
        let path = text_path
            .cloned()
            .map(TextPath::sanitized)
            .and_then(|path| match path.mode {
                TextPathMode::Follow => Some(PreparedTextPath::Follow {
                    measure: PathMeasure::new(&path.path),
                    offset: path.offset as f64,
                    flip: path.flip,
                }),
                TextPathMode::Inside => Some(PreparedTextPath::Inside {
                    inner: path.inner_bounds()?,
                    contours: path
                        .path
                        .flatten(0.35)
                        .into_iter()
                        .filter_map(|(points, closed)| closed.then_some(points))
                        .collect(),
                    inset: path.inset as f64,
                }),
            });
        Self {
            bounds,
            baseline,
            warp: warp.sanitized(),
            path,
        }
    }

    pub fn map(&self, point: Pt) -> Pt {
        let warped = map_warp(point, self.bounds, self.warp);
        match &self.path {
            None => warped,
            Some(PreparedTextPath::Inside { inner, .. }) => {
                (warped.0 + inner.x as f64, warped.1 + inner.y as f64)
            }
            Some(PreparedTextPath::Follow {
                measure,
                offset,
                flip,
            }) => {
                let Some((position, tangent)) = measure.sample(*offset + warped.0) else {
                    return warped;
                };
                let mut normal = (-tangent.1, tangent.0);
                let normal_offset = warped.1 - self.baseline as f64;
                if *flip {
                    normal = (-normal.0, -normal.1);
                }
                (
                    position.0 + normal.0 * normal_offset,
                    position.1 + normal.1 * normal_offset,
                )
            }
        }
    }

    /// Whether an already-mapped sample remains in an inside-path frame.
    pub fn includes(&self, point: Pt) -> bool {
        let Some(PreparedTextPath::Inside {
            contours, inset, ..
        }) = &self.path
        else {
            return true;
        };
        winding_contains_contours(contours, point)
            && (*inset <= 0.0 || distance_to_contours(contours, point) >= *inset)
    }
}

/// Conservative transformed bounds. Curved mappings are sampled across the
/// source rectangle rather than inferred from its corners.
pub fn mapped_bounds(
    bounds: EffectRect,
    baseline: f32,
    warp: TextWarp,
    text_path: Option<&TextPath>,
) -> EffectRect {
    let mapper = TextEffectMapper::new(bounds, baseline, warp, text_path);
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for yi in 0..=8 {
        for xi in 0..=32 {
            let p = (
                bounds.x as f64 + bounds.width as f64 * xi as f64 / 32.0,
                bounds.y as f64 + bounds.height as f64 * yi as f64 / 8.0,
            );
            let q = mapper.map(p);
            x0 = x0.min(q.0);
            y0 = y0.min(q.1);
            x1 = x1.max(q.0);
            y1 = y1.max(q.1);
        }
    }
    if !x0.is_finite() {
        return bounds;
    }
    EffectRect::new(
        x0.floor() as f32,
        y0.floor() as f32,
        (x1.ceil() - x0.floor()) as f32,
        (y1.ceil() - y0.floor()) as f32,
    )
}

fn map_warp(point: Pt, bounds: EffectRect, warp: TextWarp) -> Pt {
    let warp = warp.sanitized();
    let w = bounds.width.max(1.0) as f64;
    let h = bounds.height.max(1.0) as f64;
    let u = ((point.0 - bounds.x as f64) / w).clamp(0.0, 1.0);
    let v = ((point.1 - bounds.y as f64) / h).clamp(0.0, 1.0);
    let bend = warp.bend as f64 / 100.0;
    let mut p = point;
    match warp.style {
        WarpStyle::None => {}
        WarpStyle::Arc => {
            // Zero at the ends and maximum at the centre.
            p.1 -= bend * h * 4.0 * u * (1.0 - u);
        }
        WarpStyle::Bulge => {
            let scale = 1.0 + bend * (1.0 - (2.0 * v - 1.0).abs());
            p.0 = bounds.x as f64 + w * 0.5 + (p.0 - bounds.x as f64 - w * 0.5) * scale;
        }
        WarpStyle::Flag => {
            p.1 += bend * h * 0.5 * (u * std::f64::consts::TAU).sin();
        }
    }
    p.0 += warp.horizontal as f64 / 100.0 * w * (v - 0.5) * 0.5;
    p.1 += warp.vertical as f64 / 100.0 * h * (u - 0.5) * 0.5;
    p
}

#[derive(Debug)]
struct PathMeasure {
    points: Vec<Pt>,
    cumulative: Vec<f64>,
    length: f64,
    closed: bool,
}

impl PathMeasure {
    fn new(path: &Path) -> Self {
        let Some((mut points, closed)) = path
            .flatten(0.35)
            .into_iter()
            .find(|(points, _)| points.len() >= 2)
        else {
            return Self {
                points: Vec::new(),
                cumulative: Vec::new(),
                length: 0.0,
                closed: false,
            };
        };
        if closed && points.first() != points.last() {
            points.push(points[0]);
        }
        let mut cumulative = Vec::with_capacity(points.len());
        cumulative.push(0.0);
        for pair in points.windows(2) {
            let length = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
            cumulative.push(cumulative.last().copied().unwrap_or(0.0) + length);
        }
        let length = cumulative.last().copied().unwrap_or(0.0);
        Self {
            points,
            cumulative,
            length,
            closed,
        }
    }

    fn sample(&self, mut distance: f64) -> Option<(Pt, Pt)> {
        if self.length <= f64::EPSILON {
            return None;
        }
        distance = if self.closed {
            distance.rem_euclid(self.length)
        } else {
            distance.clamp(0.0, self.length)
        };
        let upper = self.cumulative.partition_point(|v| *v <= distance);
        let i = upper.saturating_sub(1).min(self.points.len() - 2);
        let segment = self.cumulative[i + 1] - self.cumulative[i];
        let a = self.points[i];
        let b = self.points[i + 1];
        let t = if segment <= f64::EPSILON {
            0.0
        } else {
            (distance - self.cumulative[i]) / segment
        };
        let tangent = (
            (b.0 - a.0) / segment.max(f64::EPSILON),
            (b.1 - a.1) / segment.max(f64::EPSILON),
        );
        Some(((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t), tangent))
    }
}

fn path_bounds(path: &Path) -> Option<EffectRect> {
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (points, _) in path.flatten(0.35) {
        for (x, y) in points {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0.is_finite() && x1 > x0 && y1 > y0)
        .then(|| EffectRect::new(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32))
}

fn winding_contains(path: &Path, point: Pt) -> bool {
    let contours: Vec<Vec<Pt>> = path
        .flatten(0.35)
        .into_iter()
        .filter_map(|(points, closed)| closed.then_some(points))
        .collect();
    winding_contains_contours(&contours, point)
}

fn winding_contains_contours(contours: &[Vec<Pt>], point: Pt) -> bool {
    let mut winding = 0_i32;
    for points in contours {
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.1 <= point.1 {
                if b.1 > point.1 && cross(a, b, point) > 0.0 {
                    winding += 1;
                }
            } else if b.1 <= point.1 && cross(a, b, point) < 0.0 {
                winding -= 1;
            }
        }
    }
    winding != 0
}

fn distance_to_path(path: &Path, point: Pt) -> f64 {
    let contours: Vec<Vec<Pt>> = path
        .flatten(0.35)
        .into_iter()
        .map(|(points, _)| points)
        .collect();
    distance_to_contours(&contours, point)
}

fn distance_to_contours(contours: &[Vec<Pt>], point: Pt) -> f64 {
    contours
        .iter()
        .flat_map(|points| {
            points
                .windows(2)
                .map(|pair| segment_distance(point, pair[0], pair[1]))
                .collect::<Vec<_>>()
        })
        .fold(f64::INFINITY, f64::min)
}

fn segment_distance(p: Pt, a: Pt, b: Pt) -> f64 {
    let ab = (b.0 - a.0, b.1 - a.1);
    let length2 = ab.0 * ab.0 + ab.1 * ab.1;
    if length2 <= f64::EPSILON {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * ab.0 + (p.1 - a.1) * ab.1) / length2).clamp(0.0, 1.0);
    (p.0 - (a.0 + ab.0 * t)).hypot(p.1 - (a.1 + ab.1 * t))
}

fn cross(a: Pt, b: Pt, p: Pt) -> f64 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

fn percent(value: f32) -> f32 {
    finite(value, 0.0).clamp(-100.0, 100.0)
}

fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_raster::vector::{Anchor, SubPath};

    fn line(closed: bool, points: &[(f64, f64)]) -> Path {
        Path {
            subpaths: vec![SubPath {
                anchors: points.iter().copied().map(Anchor::corner).collect(),
                closed,
            }],
        }
    }

    #[test]
    fn sanitizes_effect_values_and_old_defaults_are_identity() {
        assert!(TextWarp::default().is_identity());
        let warp = TextWarp {
            style: WarpStyle::Arc,
            bend: f32::INFINITY,
            horizontal: -400.0,
            vertical: 101.0,
        }
        .sanitized();
        assert_eq!(warp.bend, 0.0);
        assert_eq!(warp.horizontal, -100.0);
        assert_eq!(warp.vertical, 100.0);
    }

    #[test]
    fn arc_and_distortion_map_editable_points() {
        let bounds = EffectRect::new(0.0, 0.0, 100.0, 20.0);
        let q = map_point(
            (50.0, 10.0),
            bounds,
            15.0,
            TextWarp {
                style: WarpStyle::Arc,
                bend: 50.0,
                horizontal: 20.0,
                vertical: 0.0,
            },
            None,
        );
        assert!((q.0 - 50.0).abs() < 1e-6);
        assert!((q.1 - 0.0).abs() < 1e-6, "{q:?}");
    }

    #[test]
    fn follow_path_uses_offset_and_flip_without_rasterizing() {
        let path = TextPath {
            path: line(false, &[(10.0, 50.0), (110.0, 50.0)]),
            offset: 20.0,
            ..Default::default()
        };
        let bounds = EffectRect::new(0.0, 0.0, 100.0, 20.0);
        let q = map_point((10.0, 5.0), bounds, 10.0, TextWarp::default(), Some(&path));
        assert!((q.0 - 40.0).abs() < 0.01, "{q:?}");
        assert!((q.1 - 45.0).abs() < 0.01, "{q:?}");
        let flipped = TextPath { flip: true, ..path };
        let q = map_point(
            (10.0, 5.0),
            bounds,
            10.0,
            TextWarp::default(),
            Some(&flipped),
        );
        assert!((q.1 - 55.0).abs() < 0.01, "{q:?}");
    }

    #[test]
    fn inside_path_exposes_wrap_box_and_clips_to_closed_shape() {
        let text_path = TextPath {
            path: line(
                true,
                &[(10.0, 20.0), (110.0, 20.0), (110.0, 80.0), (10.0, 80.0)],
            ),
            mode: TextPathMode::Inside,
            inset: 5.0,
            ..Default::default()
        };
        assert_eq!(
            text_path.inner_bounds(),
            Some(EffectRect::new(15.0, 25.0, 90.0, 50.0))
        );
        assert!(text_path.contains((50.0, 50.0)));
        assert!(!text_path.contains((12.0, 50.0)));
        assert!(!text_path.contains((120.0, 50.0)));
    }

    #[test]
    fn effects_round_trip_with_path_geometry() {
        let effect = TextPath {
            path: line(false, &[(0.0, 0.0), (100.0, 25.0)]),
            offset: 12.5,
            flip: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&effect).unwrap();
        assert_eq!(serde_json::from_str::<TextPath>(&json).unwrap(), effect);
    }
}
