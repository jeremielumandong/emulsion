//! Bounded orthogonal routing, with obstacle-aware doglegs and a grid fallback.
use super::Bounds;
use std::{cmp::Reverse, collections::BinaryHeap};
type Point = (f64, f64);
fn distance(a: Point, b: Point) -> f64 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}
fn clear(a: Point, b: Point, obstacles: &[Bounds]) -> bool {
    !obstacles.iter().any(|&[x, y, w, h]| {
        if (a.0 - b.0).abs() < 0.001 {
            a.0 > x && a.0 < x + w && a.1.min(b.1) < y + h && a.1.max(b.1) > y
        } else if (a.1 - b.1).abs() < 0.001 {
            a.1 > y && a.1 < y + h && a.0.min(b.0) < x + w && a.0.max(b.0) > x
        } else {
            true
        }
    })
}
fn simplify(points: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for p in points {
        if out.last() == Some(&p) {
            continue;
        }
        while out.len() > 1 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            if ((a.0 - b.0) * (b.1 - p.1) - (a.1 - b.1) * (b.0 - p.0)).abs() < 0.001
                && ((b.0 - a.0) * (p.0 - b.0) + (b.1 - a.1) * (p.1 - b.1)) >= 0.
            {
                out.pop();
            } else {
                break;
            }
        }
        out.push(p);
    }
    out
}
pub fn orthogonal(
    start: Point,
    sd: Point,
    end: Point,
    ed: Point,
    bounds: &[Bounds],
) -> (Vec<Point>, bool) {
    let a = (start.0 + sd.0 * 18., start.1 + sd.1 * 18.);
    let b = (end.0 + ed.0 * 18., end.1 + ed.1 * 18.);
    let obstacles = bounds
        .iter()
        .map(|&[x, y, w, h]| [x - 6., y - 6., w + 12., h + 12.])
        .collect::<Vec<_>>();
    let mx = (a.0 + b.0) / 2.;
    let my = (a.1 + b.1) / 2.;
    let mut candidates = vec![
        vec![a, (a.0, b.1), b],
        vec![a, (b.0, a.1), b],
        vec![a, (mx, a.1), (mx, b.1), b],
        vec![a, (a.0, my), (b.0, my), b],
    ];
    // Far-away objects must not send a short connection around the whole page.
    // Use a local corridor to propose routes, then verify against every obstacle.
    let local = obstacles
        .iter()
        .copied()
        .filter(|r| {
            r[0] < a.0.max(b.0) + 96.
                && r[0] + r[2] > a.0.min(b.0) - 96.
                && r[1] < a.1.max(b.1) + 96.
                && r[1] + r[3] > a.1.min(b.1) - 96.
        })
        .collect::<Vec<_>>();
    let min_x = local.iter().map(|r| r[0]).fold(a.0.min(b.0), f64::min) - 12.;
    let max_x = local
        .iter()
        .map(|r| r[0] + r[2])
        .fold(a.0.max(b.0), f64::max)
        + 12.;
    let min_y = local.iter().map(|r| r[1]).fold(a.1.min(b.1), f64::min) - 12.;
    let max_y = local
        .iter()
        .map(|r| r[1] + r[3])
        .fold(a.1.max(b.1), f64::max)
        + 12.;
    for x in [min_x, max_x] {
        candidates.push(vec![a, (x, a.1), (x, b.1), b]);
    }
    for y in [min_y, max_y] {
        candidates.push(vec![a, (a.0, y), (b.0, y), b]);
    }
    let best = candidates
        .into_iter()
        .filter(|path| path.windows(2).all(|p| clear(p[0], p[1], &obstacles)))
        .min_by(|a, b| {
            let cost =
                |path: &Vec<Point>| path.windows(2).map(|p| distance(p[0], p[1])).sum::<f64>();
            cost(a).total_cmp(&cost(b))
        });
    let route = best
        .or_else(|| {
            grid(a, b, &local).filter(|p| p.windows(2).all(|s| clear(s[0], s[1], &obstacles)))
        })
        .or_else(|| {
            let left = obstacles.iter().map(|r| r[0]).fold(a.0.min(b.0), f64::min) - 12.;
            let top = obstacles.iter().map(|r| r[1]).fold(a.1.min(b.1), f64::min) - 12.;
            [
                vec![a, (left, a.1), (left, b.1), b],
                vec![a, (a.0, top), (b.0, top), b],
            ]
            .into_iter()
            .find(|p| p.windows(2).all(|s| clear(s[0], s[1], &obstacles)))
        })
        .or_else(|| grid(a, b, &obstacles));
    let blocked = route.is_none();
    let route = route.unwrap_or_else(|| vec![a, (mx, a.1), (mx, b.1), b]);
    (
        simplify(
            std::iter::once(start)
                .chain(route)
                .chain(std::iter::once(end))
                .collect(),
        ),
        blocked,
    )
}
fn grid(start: Point, end: Point, obstacles: &[Bounds]) -> Option<Vec<Point>> {
    let mut xs = vec![start.0, end.0];
    let mut ys = vec![start.1, end.1];
    for &[x, y, w, h] in obstacles {
        xs.extend([x - 1., x + w + 1.]);
        ys.extend([y - 1., y + h + 1.]);
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let w = xs.len();
    let h = ys.len();
    // Bound memory, not obstacle count. Dense aligned diagrams have few unique
    // coordinates even with hundreds of objects.
    if w.checked_mul(h)? > 1_000_000 {
        return None;
    }
    let mut horizontal = vec![0i32; w * h];
    let mut vertical = vec![0i32; w * h];
    for &[x, y, bw, bh] in obstacles {
        let x0 = xs.partition_point(|v| *v <= x).saturating_sub(1);
        let x1 = xs.partition_point(|v| *v < x + bw).min(w - 1);
        let y0 = ys.partition_point(|v| *v <= y).saturating_sub(1);
        let y1 = ys.partition_point(|v| *v < y + bh).min(h - 1);
        for row in ys.partition_point(|v| *v <= y)..ys.partition_point(|v| *v < y + bh) {
            horizontal[row * w + x0] += 1;
            horizontal[row * w + x1] -= 1;
        }
        for col in xs.partition_point(|v| *v <= x)..xs.partition_point(|v| *v < x + bw) {
            vertical[y0 * w + col] += 1;
            vertical[y1 * w + col] -= 1;
        }
    }
    for row in 0..h {
        for col in 1..w {
            horizontal[row * w + col] += horizontal[row * w + col - 1];
        }
    }
    for row in 1..h {
        for col in 0..w {
            vertical[row * w + col] += vertical[(row - 1) * w + col];
        }
    }
    let index = |p: Point| {
        Some(ys.iter().position(|y| *y == p.1)? * w + xs.iter().position(|x| *x == p.0)?)
    };
    let from = index(start)?;
    let to = index(end)?;
    let point = |i: usize| (xs[i % w], ys[i / w]);
    let mut costs = vec![u64::MAX; w * h];
    let mut previous = vec![usize::MAX; w * h];
    let mut queue = BinaryHeap::new();
    costs[from] = 0;
    let heuristic = |i| (distance(point(i), end) * 1000.).floor() as u64;
    queue.push(Reverse((heuristic(from), 0u64, from)));
    while let Some(Reverse((_, cost, i))) = queue.pop() {
        if cost != costs[i] {
            continue;
        }
        if i == to {
            break;
        }
        for n in [
            (i % w > 0).then(|| i - 1),
            (i % w + 1 < w).then(|| i + 1),
            (i >= w).then(|| i - w),
            (i + w < w * h).then(|| i + w),
        ]
        .into_iter()
        .flatten()
        {
            let blocked = if i / w == n / w {
                horizontal[i.min(n)] > 0
            } else {
                vertical[i.min(n)] > 0
            };
            if blocked {
                continue;
            }
            let next = cost + (distance(point(i), point(n)) * 1000.).round() as u64;
            if next < costs[n] {
                costs[n] = next;
                previous[n] = i;
                queue.push(Reverse((next.saturating_add(heuristic(n)), next, n)));
            }
        }
    }
    if costs[to] == u64::MAX {
        return None;
    }
    let mut out = vec![end];
    let mut at = to;
    while at != from {
        at = previous[at];
        out.push(point(at));
    }
    out.reverse();
    Some(out)
}

/// An exterior loop follows the attachment directions around the shape envelope.
pub(super) fn cyclical(
    start: (f64, f64),
    sd: (f64, f64),
    end: (f64, f64),
    ed: (f64, f64),
    a: [f64; 4],
    b: [f64; 4],
) -> Vec<(f64, f64)> {
    let left = a[0].min(b[0]) - 48.;
    let right = (a[0] + a[2]).max(b[0] + b[2]) + 48.;
    let top = a[1].min(b[1]) - 48.;
    let bottom = (a[1] + a[3]).max(b[1] + b[3]) + 48.;
    let side = |(x, y): (f64, f64)| {
        if x.abs() >= y.abs() {
            if x >= 0. { 0 } else { 2 }
        } else if y >= 0. {
            1
        } else {
            3
        }
    };
    let project = |p: (f64, f64), side: usize| match side {
        0 => (right, p.1),
        1 => (p.0, bottom),
        2 => (left, p.1),
        _ => (p.0, top),
    };
    let mut from = side(sd);
    let to = side(ed);
    let mut points = vec![start, project(start, from)];
    loop {
        points.push([(right, bottom), (left, bottom), (left, top), (right, top)][from]);
        from = (from + 1) % 4;
        if from == to {
            break;
        }
    }
    points.extend([project(end, to), end]);
    points.dedup();
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dense_grid_routes_and_overlaps_report_failure() {
        let obstacles = (0..120)
            .map(|i| {
                [
                    40. + (i % 12) as f64 * 30.,
                    20. + (i / 12) as f64 * 30.,
                    16.,
                    16.,
                ]
            })
            .collect::<Vec<_>>();
        let route = grid((20., 25.), (420., 295.), &obstacles).unwrap();
        assert!(route.windows(2).all(|p| clear(p[0], p[1], &obstacles)));
        let (_, blocked) = orthogonal(
            (0., 0.),
            (1., 0.),
            (100., 0.),
            (-1., 0.),
            &[[10., -20., 30., 40.]],
        );
        assert!(blocked);
    }
    #[test]
    fn indexed_grid_segments_match_geometric_checks() {
        // Varied overlapping rectangles exercise the horizontal/vertical interval boundaries.
        for seed in 0..12 {
            let obstacles = (0..90)
                .map(|i| {
                    [
                        ((i * 47 + seed * 13) % 350) as f64,
                        ((i * 83 + seed * 17) % 350) as f64,
                        12. + (i % 9) as f64,
                        19.,
                    ]
                })
                .collect::<Vec<_>>();
            let path = grid((-10., -10.), (380., 380.), &obstacles).unwrap();
            assert!(path.windows(2).all(|p| clear(p[0], p[1], &obstacles)));
        }
    }
    #[test]
    fn distant_objects_do_not_force_page_wide_detours() {
        let mut obstacles = vec![
            [20., 20., 80., 60.],
            [240., 20., 80., 60.],
            [140., 0., 60., 100.],
        ];
        obstacles.extend((0..100).map(|i| [1000. + i as f64 * 100., 1000., 50., 50.]));
        let (points, blocked) =
            orthogonal((100., 50.), (1., 0.), (240., 50.), (-1., 0.), &obstacles);
        assert!(!blocked);
        assert!(
            points
                .iter()
                .all(|p| p.0 > -100. && p.0 < 400. && p.1 > -100. && p.1 < 200.)
        );
        assert!(
            points
                .windows(2)
                .all(|p| clear(p[0], p[1], &obstacles[2..]))
        );
    }
    #[test]
    fn connectors_avoid_obstacles_and_keep_explicit_port_directions() {
        let obstacles = [
            [20., 20., 80., 60.],
            [240., 20., 80., 60.],
            [140., 0., 60., 100.],
        ];
        let (points, blocked) =
            orthogonal((100., 50.), (1., 0.), (240., 50.), (-1., 0.), &obstacles);
        assert!(!blocked);
        assert_eq!(points.first(), Some(&(100., 50.)));
        assert_eq!(points.last(), Some(&(240., 50.)));
        assert!(
            points
                .windows(2)
                .all(|p| p[0].0 == p[1].0 || p[0].1 == p[1].1)
        );
        assert!(
            points
                .windows(2)
                .all(|p| clear(p[0], p[1], &obstacles[2..]))
        );
        assert!(points[1].0 > 100.);
    }
}
