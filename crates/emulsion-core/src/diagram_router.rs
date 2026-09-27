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
pub fn midpoint(points: &[Point]) -> Point {
    let total = points.windows(2).map(|p| distance(p[0], p[1])).sum::<f64>();
    let mut left = total / 2.;
    for p in points.windows(2) {
        let d = distance(p[0], p[1]);
        if left <= d && d > 0. {
            return (
                p[0].0 + (p[1].0 - p[0].0) * left / d,
                p[0].1 + (p[1].1 - p[0].1) * left / d,
            );
        }
        left -= d;
    }
    points.first().copied().unwrap_or_default()
}
pub fn orthogonal(start: Point, sd: Point, end: Point, ed: Point, bounds: &[Bounds]) -> Vec<Point> {
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
    let min_x = obstacles.iter().map(|r| r[0]).fold(a.0.min(b.0), f64::min) - 12.;
    let max_x = obstacles
        .iter()
        .map(|r| r[0] + r[2])
        .fold(a.0.max(b.0), f64::max)
        + 12.;
    let min_y = obstacles.iter().map(|r| r[1]).fold(a.1.min(b.1), f64::min) - 12.;
    let max_y = obstacles
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
        .or_else(|| grid(a, b, &obstacles))
        .unwrap_or_else(|| vec![a, (mx, a.1), (mx, b.1), b]);
    simplify(
        std::iter::once(start)
            .chain(route)
            .chain(std::iter::once(end))
            .collect(),
    )
}
fn grid(start: Point, end: Point, obstacles: &[Bounds]) -> Option<Vec<Point>> {
    if obstacles.len() > 64 {
        return None;
    }
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
    queue.push(Reverse((0u64, from)));
    while let Some(Reverse((cost, i))) = queue.pop() {
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
            if !clear(point(i), point(n), obstacles) {
                continue;
            }
            let next = cost + (distance(point(i), point(n)) * 1000.).round() as u64;
            if next < costs[n] {
                costs[n] = next;
                previous[n] = i;
                queue.push(Reverse((next, n)));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connectors_avoid_obstacles_and_keep_explicit_port_directions() {
        let obstacles = [
            [20., 20., 80., 60.],
            [240., 20., 80., 60.],
            [140., 0., 60., 100.],
        ];
        let points = orthogonal((100., 50.), (1., 0.), (240., 50.), (-1., 0.), &obstacles);
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
