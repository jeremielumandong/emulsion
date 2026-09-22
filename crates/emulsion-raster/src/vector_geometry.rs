//! Editable shape primitives and non-zero-winding vector Boolean operations.
//!
//! Boolean clipping approximates cubic curves by line segments within 0.1
//! document pixels. The result stays an editable path, including hole contours;
//! it is never converted to a raster mask. Primitive ellipses keep cubic handles.

use crate::vector::{Anchor, MAX_ANCHORS, Path, Pt, SubPath};
use i_overlay::core::{fill_rule::FillRule, overlay_rule::OverlayRule};
use i_overlay::float::single::SingleFloatOverlay;

// Leave room within the public 0.1px tolerance for clipping quantization.
const TOLERANCE: f64 = 0.05;
const MAX_FLATTENED: usize = 8_192;
const MAX_COORD: f64 = 1_000_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BooleanOp {
    #[default]
    Add,
    Subtract,
    Intersect,
    Exclude,
}

/// Closed clockwise rectangle. Negative dimensions are normalized.
pub fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Path {
    let Some((x, y, w, h)) = normalized_rect(x, y, width, height) else {
        return Path::default();
    };
    Path {
        subpaths: vec![SubPath {
            anchors: [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
                .into_iter()
                .map(Anchor::corner)
                .collect(),
            closed: true,
        }],
    }
}

/// Four editable cubic arcs using the standard kappa ellipse approximation.
pub fn ellipse(x: f64, y: f64, width: f64, height: f64) -> Path {
    let Some((x, y, w, h)) = normalized_rect(x, y, width, height) else {
        return Path::default();
    };
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (cx, cy) = (x + rx, y + ry);
    let k = 4.0 * (2.0_f64.sqrt() - 1.0) / 3.0;
    let (kx, ky) = (k * rx, k * ry);
    let points = [
        ((cx, y), (cx - kx, y), (cx + kx, y)),
        ((x + w, cy), (x + w, cy - ky), (x + w, cy + ky)),
        ((cx, y + h), (cx + kx, y + h), (cx - kx, y + h)),
        ((x, cy), (x, cy + ky), (x, cy - ky)),
    ];
    Path {
        subpaths: vec![SubPath {
            anchors: points
                .into_iter()
                .map(|(p, h_in, h_out)| Anchor {
                    p,
                    h_in,
                    h_out,
                    smooth: true,
                })
                .collect(),
            closed: true,
        }],
    }
}

fn normalized_rect(x: f64, y: f64, w: f64, h: f64) -> Option<(f64, f64, f64, f64)> {
    if ![x, y, w, h, x + w, y + h]
        .into_iter()
        .all(|v| v.is_finite() && v.abs() <= MAX_COORD)
        || w == 0.0
        || h == 0.0
    {
        return None;
    }
    Some((x.min(x + w), y.min(y + h), w.abs(), h.abs()))
}

/// Exact geometric `(x, y, width, height)` bounds of cubic paths, without
/// stroke padding. Unused handles on open endpoints do not enlarge the bounds.
pub fn bounds(path: &Path) -> Option<(f64, f64, f64, f64)> {
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut include = |p: Pt| {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
        x1 = x1.max(p.0);
        y1 = y1.max(p.1);
    };
    for sub in &path.subpaths {
        for anchor in &sub.anchors {
            if ![anchor.p, anchor.h_in, anchor.h_out]
                .into_iter()
                .all(|p| p.0.is_finite() && p.1.is_finite())
            {
                return None;
            }
            include(anchor.p);
        }
        let count = if sub.closed {
            sub.anchors.len()
        } else {
            sub.anchors.len().saturating_sub(1)
        };
        for i in 0..count {
            let a = sub.anchors[i];
            let b = sub.anchors[(i + 1) % sub.anchors.len()];
            for coord in [
                [a.p.0, a.h_out.0, b.h_in.0, b.p.0],
                [a.p.1, a.h_out.1, b.h_in.1, b.p.1],
            ] {
                for t in extrema(coord).into_iter().flatten() {
                    if t > 0.0 && t < 1.0 {
                        include(crate::vector::cubic_at(a.p, a.h_out, b.h_in, b.p, t));
                    }
                }
            }
        }
    }
    (x0 <= x1 && y0 <= y1).then_some((x0, y0, x1 - x0, y1 - y0))
}

fn extrema(p: [f64; 4]) -> [Option<f64>; 2] {
    let a = -p[0] + 3.0 * p[1] - 3.0 * p[2] + p[3];
    let b = 2.0 * (p[0] - 2.0 * p[1] + p[2]);
    let c = p[1] - p[0];
    if a.abs() <= f64::EPSILON * (b.abs() + c.abs()).max(1.0) {
        return [if b == 0.0 { None } else { Some(-c / b) }, None];
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return [None, None];
    }
    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
    if q == 0.0 {
        [Some(-b / (2.0 * a)), None]
    } else {
        [Some(q / a), Some(c / q)]
    }
}

/// Combine closed paths in the same coordinate system. Open paths are rejected
/// rather than implicitly closing an editable pen stroke. Empty paths are valid.
/// Excessively complex geometry returns an error before clipping allocations.
pub fn boolean(a: &Path, b: &Path, op: BooleanOp) -> Result<Path, String> {
    let mut budget = MAX_FLATTENED;
    let subject = contours(a, &mut budget)?;
    let clip = contours(b, &mut budget)?;
    check_intersection_budget(&subject, &clip)?;
    let rule = match op {
        BooleanOp::Add => OverlayRule::Union,
        BooleanOp::Subtract => OverlayRule::Difference,
        BooleanOp::Intersect => OverlayRule::Intersect,
        BooleanOp::Exclude => OverlayRule::Xor,
    };
    // The i64 engine retains subpixel precision for widely separated shapes.
    let shapes = subject.overlay_as::<i64>(&clip, rule, FillRule::NonZero);
    let count: usize = shapes.iter().flatten().map(Vec::len).sum();
    if count > MAX_ANCHORS {
        return Err("The combined shape has too many anchors to edit safely".into());
    }
    Ok(Path {
        subpaths: shapes
            .into_iter()
            .flatten()
            .filter(|c| c.len() >= 3)
            .map(|c| SubPath {
                anchors: c.into_iter().map(|[x, y]| Anchor::corner((x, y))).collect(),
                closed: true,
            })
            .collect(),
    })
}

fn contours(path: &Path, budget: &mut usize) -> Result<Vec<Vec<[f64; 2]>>, String> {
    if path.anchor_count() > MAX_ANCHORS || path.subpaths.len() > MAX_ANCHORS {
        return Err("The shape has too many path components or anchors".into());
    }
    let mut result = Vec::new();
    for sub in &path.subpaths {
        if sub.anchors.is_empty() {
            continue;
        }
        if !sub.closed {
            return Err("Close the path before combining shapes".into());
        }
        for anchor in &sub.anchors {
            if ![anchor.p, anchor.h_in, anchor.h_out].into_iter().all(|p| {
                p.0.is_finite()
                    && p.1.is_finite()
                    && p.0.abs() <= MAX_COORD
                    && p.1.abs() <= MAX_COORD
            }) {
                return Err("The shape contains invalid or out-of-range coordinates".into());
            }
        }
        if sub.anchors.len() < 2 {
            continue;
        }
        let mut contour = Vec::new();
        push_point(&mut contour, sub.anchors[0].p, budget)?;
        for i in 0..sub.anchors.len() {
            let a = sub.anchors[i];
            let b = sub.anchors[(i + 1) % sub.anchors.len()];
            flatten([a.p, a.h_out, b.h_in, b.p], 0, &mut contour, budget)?;
        }
        if contour.first() == contour.last() {
            contour.pop();
        }
        if contour.len() >= 3 {
            result.push(contour);
        }
    }
    Ok(result)
}

fn push_point(out: &mut Vec<[f64; 2]>, p: Pt, budget: &mut usize) -> Result<(), String> {
    if out.last() == Some(&[p.0, p.1]) {
        return Ok(());
    }
    if *budget == 0 {
        return Err("The shape is too complex to combine at subpixel precision".into());
    }
    *budget -= 1;
    out.push([p.0, p.1]);
    Ok(())
}

fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let d = (b.0 - a.0, b.1 - a.1);
    let len2 = d.0 * d.0 + d.1 * d.1;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * d.0 + (p.1 - a.1) * d.1) / len2).clamp(0.0, 1.0)
    };
    (p.0 - a.0 - t * d.0).hypot(p.1 - a.1 - t * d.1)
}

fn flatten(
    p: [Pt; 4],
    depth: usize,
    out: &mut Vec<[f64; 2]>,
    budget: &mut usize,
) -> Result<(), String> {
    if distance_to_segment(p[1], p[0], p[3]).max(distance_to_segment(p[2], p[0], p[3])) <= TOLERANCE
    {
        return push_point(out, p[3], budget);
    }
    if depth >= 24 {
        return Err("The curve cannot be combined at subpixel precision".into());
    }
    let mid = |a: Pt, b: Pt| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    let (a, b, c) = (mid(p[0], p[1]), mid(p[1], p[2]), mid(p[2], p[3]));
    let (d, e) = (mid(a, b), mid(b, c));
    let m = mid(d, e);
    flatten([p[0], a, d, m], depth + 1, out, budget)?;
    flatten([m, e, c, p[3]], depth + 1, out, budget)
}

// Conservatively cap potentially intersecting segment pairs, including
// self-intersections, to bound the overlay graph's worst-case memory use.
fn check_intersection_budget(a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>]) -> Result<(), String> {
    let mut bounds = Vec::new();
    for contour in a.iter().chain(b) {
        for i in 0..contour.len() {
            let (p, q) = (contour[i], contour[(i + 1) % contour.len()]);
            bounds.push([
                p[0].min(q[0]),
                p[1].min(q[1]),
                p[0].max(q[0]),
                p[1].max(q[1]),
            ]);
        }
    }
    bounds.sort_unstable_by(|a, b| a[0].total_cmp(&b[0]));
    let mut candidates = 0;
    for (i, a) in bounds.iter().enumerate() {
        for b in &bounds[i + 1..] {
            if b[0] > a[2] {
                break;
            }
            if b[1] <= a[3] && b[3] >= a[1] {
                candidates += 1;
                if candidates > 100_000 {
                    return Err(
                        "The shape has too many intersecting segments to combine safely".into(),
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(path: &Path) -> f64 {
        path.subpaths
            .iter()
            .map(|s| {
                let p = s.flatten(0.1);
                p.windows(2)
                    .map(|q| q[0].0 * q[1].1 - q[1].0 * q[0].1)
                    .sum::<f64>()
                    * 0.5
            })
            .sum::<f64>()
            .abs()
    }

    #[test]
    fn editable_primitives_normalize_bounds_and_keep_ellipse_handles() {
        let r = rectangle(20.0, 30.0, -20.0, -30.0);
        assert_eq!(r.anchor_count(), 4);
        assert_eq!(area(&r), 600.0);
        let e = ellipse(0.0, 0.0, 40.0, 20.0);
        assert_eq!(e.anchor_count(), 4);
        assert!(
            e.subpaths[0]
                .anchors
                .iter()
                .all(|a| a.smooth && a.has_handles())
        );
        assert!((area(&e) - std::f64::consts::PI * 200.0).abs() < 1.0);
        assert!(ellipse(f64::NAN, 0.0, 10.0, 10.0).is_empty());
        assert!(rectangle(0.0, 0.0, 0.0, 10.0).is_empty());
    }

    #[test]
    fn overlapping_rectangles_all_operations() {
        let a = rectangle(0.0, 0.0, 10.0, 10.0);
        let b = rectangle(5.0, 0.0, 10.0, 10.0);
        for (op, expected) in [
            (BooleanOp::Add, 150.0),
            (BooleanOp::Subtract, 50.0),
            (BooleanOp::Intersect, 50.0),
            (BooleanOp::Exclude, 100.0),
        ] {
            let result = boolean(&a, &b, op).unwrap();
            assert!((area(&result) - expected).abs() < 0.001, "{op:?}");
            assert!(result.subpaths.iter().all(|s| s.closed));
        }
    }

    #[test]
    fn disjoint_identical_empty_and_edge_touching_operations() {
        let a = rectangle(0.0, 0.0, 10.0, 10.0);
        let b = rectangle(20.0, 0.0, 10.0, 10.0);
        for (op, expected) in [
            (BooleanOp::Add, 200.0),
            (BooleanOp::Subtract, 100.0),
            (BooleanOp::Intersect, 0.0),
            (BooleanOp::Exclude, 200.0),
        ] {
            assert!((area(&boolean(&a, &b, op).unwrap()) - expected).abs() < 0.001);
        }
        for op in [BooleanOp::Subtract, BooleanOp::Exclude] {
            assert!(boolean(&a, &a, op).unwrap().is_empty());
        }
        assert!(
            (area(&boolean(&a, &Path::default(), BooleanOp::Add).unwrap()) - 100.0).abs() < 0.001
        );
        let touching = rectangle(10.0, 0.0, 10.0, 10.0);
        assert!(
            boolean(&a, &touching, BooleanOp::Intersect)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn subtraction_preserves_holes_in_followup_operations() {
        let outer = rectangle(0.0, 0.0, 20.0, 20.0);
        let inner = rectangle(5.0, 5.0, 10.0, 10.0);
        let ring = boolean(&outer, &inner, BooleanOp::Subtract).unwrap();
        assert_eq!(ring.subpaths.len(), 2);
        assert!((area(&ring) - 300.0).abs() < 0.001);
        assert!(
            boolean(&ring, &inner, BooleanOp::Intersect)
                .unwrap()
                .is_empty()
        );
        assert!((area(&boolean(&ring, &inner, BooleanOp::Add).unwrap()) - 400.0).abs() < 0.001);
    }

    #[test]
    fn ellipse_clipping_is_editable_and_accurate() {
        let e = ellipse(0.0, 0.0, 100.0, 100.0);
        let half = rectangle(50.0, 0.0, 50.0, 100.0);
        let result = boolean(&e, &half, BooleanOp::Intersect).unwrap();
        assert!(
            (area(&result) - std::f64::consts::PI * 1250.0).abs() < 5.0,
            "actual area: {}",
            area(&result)
        );
        assert!(
            result
                .subpaths
                .iter()
                .flat_map(|s| &s.anchors)
                .all(|a| !a.has_handles())
        );
        assert!(result.anchor_count() < 200);
    }

    #[test]
    fn rejects_open_invalid_and_excessive_geometry() {
        let mut p = rectangle(0.0, 0.0, 10.0, 10.0);
        p.subpaths[0].closed = false;
        assert!(boolean(&p, &Path::default(), BooleanOp::Add).is_err());
        p.subpaths[0].closed = true;
        p.subpaths[0].anchors[0].h_in.0 = f64::INFINITY;
        assert!(boolean(&p, &Path::default(), BooleanOp::Add).is_err());
        let excessive = Path {
            subpaths: vec![SubPath {
                anchors: vec![Anchor::corner((0.0, 0.0)); MAX_ANCHORS + 1],
                closed: true,
            }],
        };
        assert!(boolean(&excessive, &Path::default(), BooleanOp::Add).is_err());
    }

    #[test]
    fn exact_bounds_use_cubic_extrema_not_handle_box() {
        let p = Path {
            subpaths: vec![SubPath {
                anchors: vec![
                    Anchor {
                        p: (0.0, 0.0),
                        h_in: (-100.0, -100.0),
                        h_out: (0.0, 10.0),
                        smooth: false,
                    },
                    Anchor {
                        p: (10.0, 0.0),
                        h_in: (10.0, 10.0),
                        h_out: (100.0, 100.0),
                        smooth: false,
                    },
                ],
                closed: false,
            }],
        };
        assert_eq!(bounds(&p), Some((0.0, 0.0, 10.0, 7.5)));
        assert_eq!(
            bounds(&ellipse(3.0, 5.0, 20.0, 40.0)),
            Some((3.0, 5.0, 20.0, 40.0))
        );
        assert_eq!(bounds(&Path::default()), None);
    }
}
