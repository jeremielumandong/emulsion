//! Conservative simplicity certificate for a single PSD cubic contour.
//!
//! This is not a Boolean/path-intersection engine. Every retained cubic piece
//! is strictly monotone in a coordinate, hence cannot cross itself. Pairs of
//! pieces must have disjoint convex control hulls after bounded subdivision;
//! neighbors may share only their authored endpoint, separated by a strict
//! half-plane. Cubic curves lie in their control hulls. Any inconclusive case,
//! numerical near-contact or exhausted work budget refuses editable exchange.
//!
//! The caller supplies 8.24-grid coordinates in [-16, 16). Across every stage
//! of the certificate, each curve can be halved at most eight times. The exact
//! dyadic coordinates then have at most 52 significant bits (53 during sums),
//! so all subdivision, endpoint equality and monotonicity checks are exact in
//! f64. Projection arithmetic uses a separate conservative separation margin.

use ag_psd::psd::BezierPath;
type Point = [f64; 2];
type Curve = [Point; 4];
const EPS: f64 = 1e-10;
const MAX_PIECES: usize = 4096;
const MAX_WORK: usize = 200_000;
const MAX_DEPTH: u8 = 8;

#[derive(Clone, Copy)]
struct Piece {
    curve: Curve,
    depth: u8,
}

impl Piece {
    fn split(self) -> Option<(Self, Self)> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        let (a, b) = split(self.curve);
        let depth = self.depth + 1;
        Some((Self { curve: a, depth }, Self { curve: b, depth }))
    }
}

fn midpoint(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
fn split(c: Curve) -> (Curve, Curve) {
    let a = midpoint(c[0], c[1]);
    let b = midpoint(c[1], c[2]);
    let d = midpoint(c[2], c[3]);
    let e = midpoint(a, b);
    let f = midpoint(b, d);
    let m = midpoint(e, f);
    ([c[0], a, e, m], [m, f, d, c[3]])
}
fn monotone(c: &Curve) -> bool {
    (0..2).any(|axis| {
        c[0][axis] != c[3][axis]
            && (c.windows(2).all(|p| p[0][axis] <= p[1][axis])
                || c.windows(2).all(|p| p[0][axis] >= p[1][axis]))
    })
}
fn pieces(piece: Piece, output: &mut Vec<Piece>) -> bool {
    let c = piece.curve;
    if c.iter().all(|p| *p == c[0]) {
        return true;
    }
    if output.len() >= MAX_PIECES {
        return false;
    }
    if monotone(&c) {
        output.push(piece);
        return true;
    }
    let Some((a, b)) = piece.split() else {
        return false;
    };
    pieces(a, output) && pieces(b, output)
}
fn bounds(c: &Curve) -> [f64; 4] {
    c.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |mut b, p| {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
            b
        },
    )
}
fn unit(v: Point) -> Option<Point> {
    let length = v[0].hypot(v[1]);
    (length > EPS).then(|| [v[0] / length, v[1] / length])
}
fn projection(p: Point, n: Point) -> f64 {
    p[0] * n[0] + p[1] * n[1]
}
fn hulls_separate(a: &Curve, b: &Curve) -> bool {
    // Testing every control-polygon chord includes every convex-hull edge.
    // Any separating projection is sufficient; no hull construction is needed.
    [a, b].into_iter().any(|c| {
        (0..4).any(|i| {
            (i + 1..4).any(|j| {
                let Some(n) = unit([c[j][1] - c[i][1], c[i][0] - c[j][0]]) else {
                    return false;
                };
                let range = |curve: &Curve| {
                    curve
                        .iter()
                        .map(|p| projection(*p, n))
                        .fold([f64::INFINITY, f64::NEG_INFINITY], |r, p| {
                            [r[0].min(p), r[1].max(p)]
                        })
                };
                let x = range(a);
                let y = range(b);
                x[1] + EPS < y[0] || y[1] + EPS < x[0]
            })
        })
    })
}
fn endpoint_separates(a: &Curve, b: &Curve, common: Point) -> bool {
    let relative = |p: Point| [p[0] - common[0], p[1] - common[1]];
    a.iter().filter_map(|p| unit(relative(*p))).any(|left| {
        b.iter().filter_map(|p| unit(relative(*p))).any(|right| {
            let Some(n) = unit([right[0] - left[0], right[1] - left[1]]) else {
                return false;
            };
            // Strict signs for every non-endpoint control point prove that the
            // hulls meet only at the common endpoint, not along a tangent edge.
            a.iter()
                .all(|p| *p == common || projection(relative(*p), n) < -EPS)
                && b.iter()
                    .all(|p| *p == common || projection(relative(*p), n) > EPS)
        })
    })
}
fn separate(a: Piece, b: Piece, common: Option<Point>, work: &mut usize) -> bool {
    *work += 1;
    if *work > MAX_WORK {
        return false;
    }
    let x = bounds(&a.curve);
    let y = bounds(&b.curve);
    if x[2] + EPS < y[0]
        || y[2] + EPS < x[0]
        || x[3] + EPS < y[1]
        || y[3] + EPS < x[1]
        || hulls_separate(&a.curve, &b.curve)
        || common.is_some_and(|p| endpoint_separates(&a.curve, &b.curve, p))
    {
        return true;
    }
    if a.depth == MAX_DEPTH && b.depth == MAX_DEPTH {
        return false;
    }
    let extent = |b: [f64; 4]| (b[2] - b[0]).max(b[3] - b[1]);
    let carries = |c: &Piece| common.filter(|p| c.curve[0] == *p || c.curve[3] == *p);
    if a.depth < MAX_DEPTH && (b.depth == MAX_DEPTH || extent(x) >= extent(y)) {
        let Some((a0, a1)) = a.split() else {
            return false;
        };
        separate(a0, b, carries(&a0), work) && separate(a1, b, carries(&a1), work)
    } else {
        let Some((b0, b1)) = b.split() else {
            return false;
        };
        separate(a, b0, carries(&b0), work) && separate(a, b1, carries(&b1), work)
    }
}

pub(super) fn is_simple(path: &BezierPath) -> bool {
    let point = |i: usize, at: usize| [path.knots[i].points[at], path.knots[i].points[at + 1]];
    let count = path.knots.len();
    if count < 2 {
        return false;
    }
    let mut curves = Vec::new();
    for i in 0..count {
        let j = (i + 1) % count;
        let a = point(i, 2);
        let d = point(j, 2);
        let curve = if path.open && j == 0 {
            [a, a, d, d]
        } else {
            [a, point(i, 4), point(j, 0), d]
        };
        if !pieces(Piece { curve, depth: 0 }, &mut curves) {
            return false;
        }
    }
    if curves.len() < 2 {
        return false;
    }
    if curves.len() == 2 {
        // A two-arc lens has two shared endpoints. Split one monotone arc so
        // every pair in the certificate has at most one topological neighbor.
        let Some((first, second)) = curves[0].split() else {
            return false;
        };
        if !monotone(&first.curve) || !monotone(&second.curve) {
            return false;
        }
        curves[0] = first;
        curves.insert(1, second);
    }
    let boxes: Vec<_> = curves.iter().map(|piece| bounds(&piece.curve)).collect();
    let mut indices: Vec<_> = (0..curves.len()).collect();
    indices.sort_by(|a, b| boxes[*a][0].total_cmp(&boxes[*b][0]));
    let mut work = 0;
    for (at, &i) in indices.iter().enumerate() {
        for &j in &indices[at + 1..] {
            if boxes[j][0] > boxes[i][2] + EPS {
                break;
            }
            let common = if (i + 1) % curves.len() == j {
                Some(curves[i].curve[3])
            } else if (j + 1) % curves.len() == i {
                Some(curves[j].curve[3])
            } else {
                None
            };
            if !separate(curves[i], curves[j], common, &mut work) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ag_psd::psd::{BezierKnot, BooleanOperation, FillRule};

    fn knot(incoming: Point, anchor: Point, outgoing: Point) -> BezierKnot {
        BezierKnot {
            linked: false,
            points: vec![
                incoming[0],
                incoming[1],
                anchor[0],
                anchor[1],
                outgoing[0],
                outgoing[1],
            ],
        }
    }

    fn path(knots: Vec<BezierKnot>, open: bool) -> BezierPath {
        BezierPath {
            open,
            operation: Some(BooleanOperation::Combine),
            fill_rule: FillRule::EvenOdd,
            knots,
        }
    }

    fn polygon(points: &[Point]) -> BezierPath {
        path(points.iter().map(|&p| knot(p, p, p)).collect(), false)
    }

    fn notch(gap: f64) -> BezierPath {
        polygon(&[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.5 + gap, 1.0],
            [0.5 + gap, 0.25],
            [0.5, 0.25],
            [0.5, 1.0],
            [0.0, 1.0],
        ])
    }

    #[test]
    fn accepts_curved_ellipse() {
        let k = 0.552_284_749_830_793_6;
        let ellipse = path(
            vec![
                knot([1.0, -k], [1.0, 0.0], [1.0, k]),
                knot([k, 1.0], [0.0, 1.0], [-k, 1.0]),
                knot([-1.0, k], [-1.0, 0.0], [-1.0, -k]),
                knot([-k, -1.0], [0.0, -1.0], [k, -1.0]),
            ],
            false,
        );
        assert!(is_simple(&ellipse));
    }

    #[test]
    fn accepts_concave_polygon_in_either_orientation() {
        let mut vertices = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.5, 0.4], [0.0, 1.0]];
        assert!(is_simple(&polygon(&vertices)));
        vertices.reverse();
        assert!(is_simple(&polygon(&vertices)));
    }

    #[test]
    fn accepts_two_knot_lens_with_two_shared_endpoints() {
        let lens = path(
            vec![
                knot([0.25, -1.0], [0.0, 0.0], [0.25, 1.0]),
                knot([0.75, 1.0], [1.0, 0.0], [0.75, -1.0]),
            ],
            false,
        );
        assert!(is_simple(&lens));
    }

    #[test]
    fn accepts_open_curve_with_implicit_straight_closure() {
        // Unused incoming/outgoing handles must not shape the closing segment.
        let open = path(
            vec![
                knot([9.0, 9.0], [0.0, 0.0], [0.25, 1.0]),
                knot([0.75, 1.0], [1.0, 0.0], [-9.0, -9.0]),
            ],
            true,
        );
        assert!(is_simple(&open));
    }

    #[test]
    fn rejects_bowtie_crossing_and_figure_eight_contact() {
        assert!(!is_simple(&polygon(&[
            [0.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [1.0, 0.0],
        ])));
        assert!(!is_simple(&polygon(&[
            [0.0, 0.0],
            [-1.0, 1.0],
            [-2.0, 0.0],
            [-1.0, -1.0],
            [0.0, 0.0],
            [1.0, 1.0],
            [2.0, 0.0],
            [1.0, -1.0],
        ])));
    }

    #[test]
    fn rejects_smooth_interior_tangency_to_nonadjacent_edge() {
        // The cubic has y(t) = 3 * (2t - 1)^2 and touches the closing
        // horizontal edge at (0, 0), without crossing that edge.
        let tangent = path(
            vec![
                knot([-3.0, 3.0], [-3.0, 3.0], [-1.0, -1.0]),
                knot([1.0, -1.0], [3.0, 3.0], [3.0, 3.0]),
                knot([3.0, 0.0], [3.0, 0.0], [3.0, 0.0]),
                knot([-3.0, 0.0], [-3.0, 0.0], [-3.0, 0.0]),
            ],
            false,
        );
        assert!(!is_simple(&tangent));
    }

    #[test]
    fn rejects_twice_wound_identical_contour() {
        // Nonzero fills this square; even-odd cancels it completely.
        assert!(!is_simple(&polygon(&[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ])));
    }

    #[test]
    fn rejects_loop_inside_single_cubic() {
        // The first cubic crosses itself at t = (1 +/- sqrt(0.84)) / 2.
        let looped = path(
            vec![
                knot([0.0, 0.0], [0.0, 0.0], [1.0, 1.0]),
                knot([-1.0, 1.0], [0.25, 0.0], [0.25, 0.0]),
            ],
            true,
        );
        assert!(!is_simple(&looped));
    }

    #[test]
    fn rejects_retracing_and_zero_area_contours() {
        assert!(!is_simple(&polygon(&[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.5, 1.0],
            [0.5, 1.5],
            [0.5, 1.0],
            [0.0, 1.0],
        ])));
        assert!(!is_simple(&polygon(&[[0.0, 0.0], [1.0, 0.0]])));
        assert!(!is_simple(&polygon(&[[0.0, 0.0], [0.5, 0.0], [1.0, 0.0]])));
        assert!(!is_simple(&polygon(&[[0.5, 0.5]; 4])));
    }

    #[test]
    fn accepts_authored_stationary_knot_in_simple_contour() {
        // A genuinely constant cubic changes neither geometry nor winding.
        assert!(is_simple(&polygon(&[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ])));
    }

    #[test]
    fn grid_spaced_near_contact_is_supported_but_uncertainty_falls_back() {
        assert!(is_simple(&notch(1.0 / 16_777_216.0)));
        assert!(!is_simple(&notch(EPS * 0.25)));
        assert!(!is_simple(&notch(0.0)));
    }

    #[test]
    fn quantization_can_remove_simplicity_and_must_be_checked_after_rounding() {
        let mut almost_touching = notch(1e-9);
        assert!(is_simple(&almost_touching));
        for knot in &mut almost_touching.knots {
            for coordinate in &mut knot.points {
                *coordinate = (*coordinate * 16_777_216.0).round() / 16_777_216.0;
            }
        }
        assert!(!is_simple(&almost_touching));
    }

    #[test]
    fn accepts_smallest_grid_square_near_positive_range_limit() {
        let grid = 1.0 / 16_777_216.0;
        let low = 16.0 - 2.0 * grid;
        let high = 16.0 - grid;
        assert!(is_simple(&polygon(&[
            [low, low],
            [high, low],
            [high, high],
            [low, high],
        ])));
    }

    #[test]
    fn rejects_contour_exceeding_piece_budget() {
        let points: Vec<_> = (0..=MAX_PIECES)
            .map(|i| {
                let angle = std::f64::consts::TAU * i as f64 / (MAX_PIECES + 1) as f64;
                [angle.cos(), angle.sin()]
            })
            .collect();
        assert!(!is_simple(&polygon(&points)));
    }

    #[test]
    fn subdivision_is_exact_through_shared_eight_level_budget() {
        let high = (1i128 << 28) - 1;
        let low = -(1i128 << 28);
        let original = [[low, low], [high, high], [low, high], [high, low]];
        let curve = original.map(|point| point.map(|n| n as f64 / 16_777_216.0));
        let mut level = vec![(Piece { curve, depth: 0 }, original)];
        for depth in 1..=MAX_DEPTH {
            let mut next = Vec::new();
            for (piece, integer) in level {
                let (left, right) = piece.split().unwrap();
                let mut exact_left = [[0i128; 2]; 4];
                let mut exact_right = [[0i128; 2]; 4];
                for axis in 0..2 {
                    let [a, b, c, d] = integer.map(|point| point[axis]);
                    let midpoint = a + 3 * b + 3 * c + d;
                    exact_left[0][axis] = 8 * a;
                    exact_left[1][axis] = 4 * (a + b);
                    exact_left[2][axis] = 2 * (a + 2 * b + c);
                    exact_left[3][axis] = midpoint;
                    exact_right[0][axis] = midpoint;
                    exact_right[1][axis] = 2 * (b + 2 * c + d);
                    exact_right[2][axis] = 4 * (c + d);
                    exact_right[3][axis] = 8 * d;
                }
                let denominator = (1u64 << (24 + 3 * u32::from(depth))) as f64;
                for (child, exact) in [(left, exact_left), (right, exact_right)] {
                    assert_eq!(child.depth, depth);
                    assert_eq!(
                        child.curve,
                        exact.map(|point| point.map(|n| n as f64 / denominator))
                    );
                    next.push((child, exact));
                }
            }
            level = next;
        }
        assert!(level.iter().all(|(piece, _)| piece.split().is_none()));
    }
}
