//! ShapeSheet arcs in local coordinates. See Microsoft ArcTo/EllipticalArcTo
//! documentation: the control point lies on the arc, rather than being a Bezier handle.
use glam::{DAffine2, DVec2, dvec2};
use std::fmt::Write;

pub(super) fn ellipse_arc(
    start: DVec2,
    through: DVec2,
    end: DVec2,
    angle: f64,
    ratio: f64,
    scale: DVec2,
) -> Option<String> {
    if !(1e-6..=1000.).contains(&ratio) {
        return None;
    }
    let normalize = DAffine2::from_scale(dvec2(1., ratio)) * DAffine2::from_angle(-angle);
    let [a, b, c] = [start, through, end].map(|p| normalize.transform_point2(p));
    let u = b - a;
    let v = c - a;
    let det = 2. * u.perp_dot(v);
    if det.abs() < 1e-12 {
        return None;
    }
    let center = a + dvec2(
        v.y * u.length_squared() - u.y * v.length_squared(),
        u.x * v.length_squared() - v.x * u.length_squared(),
    ) / det;
    let radius = (a - center).length();
    if !radius.is_finite() || radius > 1e6 {
        return None;
    }
    let theta = |p: DVec2| (p.y - center.y).atan2(p.x - center.x);
    let from = theta(a);
    let tau = std::f64::consts::TAU;
    let mut sweep = (theta(c) - from).rem_euclid(tau);
    if (theta(b) - from).rem_euclid(tau) > sweep {
        sweep -= tau;
    }
    let count = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.) as usize;
    let step = sweep / count as f64;
    let transform = DAffine2::from_scale(scale) * normalize.inverse();
    let at = |t: f64| center + dvec2(t.cos(), t.sin()) * radius;
    let tangent = |t: f64| dvec2(-t.sin(), t.cos()) * radius;
    let mut svg = String::new();
    for i in 0..count {
        let t = from + i as f64 * step;
        let next = t + step;
        let k = 4. / 3. * (step / 4.).tan();
        let [p, q, r] = [
            at(t) + tangent(t) * k,
            at(next) - tangent(next) * k,
            at(next),
        ]
        .map(|p| transform.transform_point2(p));
        write!(svg, "C {} {} {} {} {} {} ", p.x, p.y, q.x, q.y, r.x, r.y).unwrap();
    }
    Some(svg)
}

/// Only evaluated numeric ShapeSheet formulas are interpreted; no macro/code execution.
pub(super) fn formula(text: &str, name: &str) -> Option<Vec<f64>> {
    let text = text.trim();
    let (head, args) = text.split_once('(')?;
    if !head.trim().eq_ignore_ascii_case(name) {
        return None;
    }
    let args = args.strip_suffix(')')?;
    let values = args
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && v.abs() <= 1e6)
        })
        .collect::<Option<Vec<_>>>()?;
    (values.len() <= 16384).then_some(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn elliptical_arc_retains_curvature_endpoints_and_nonuniform_scale() {
        let s = ellipse_arc(
            dvec2(2., 0.),
            dvec2(0., 1.),
            dvec2(-2., 0.),
            0.,
            2.,
            dvec2(3., 4.),
        )
        .unwrap();
        let p = emulsion_raster::vector::Path::from_svg(&format!("M 6 0 {s}")).unwrap();
        let points = p.flatten(0.001)[0].0.clone();
        assert!(points.iter().any(|p| p.1 > 3.99));
        assert!((points.last().unwrap().0 + 6.).abs() < 1e-6);
        assert!(points.last().unwrap().1.abs() < 1e-6);
    }
    #[test]
    fn collinear_arc_and_executable_formulas_are_rejected() {
        assert!(
            ellipse_arc(
                dvec2(0., 0.),
                dvec2(1., 0.),
                dvec2(2., 0.),
                0.,
                1.,
                DVec2::ONE
            )
            .is_none()
        );
        assert!(formula("POLYLINE(0,0,GUARD(Width),1)", "POLYLINE").is_none());
        assert_eq!(
            formula("POLYLINE (0, 0, .2, 1)", "POLYLINE").unwrap(),
            [0., 0., 0.2, 1.]
        );
    }
}

/// Evaluate numeric ShapeSheet NURBS using homogeneous de Boor interpolation.
/// Polynomial and rational spans become adaptively fitted cubic segments.
pub(super) fn nurbs(
    start: DVec2,
    end: DVec2,
    values: &[f64],
    ends: [f64; 4],
    size: DVec2,
    scale: DVec2,
) -> Option<String> {
    if values.len() < 8 || values.len() % 4 != 0 {
        return None;
    }
    let degree = values[1] as usize;
    if !(1..=25).contains(&degree) || values[1] != degree as f64 {
        return None;
    }
    let mut points = vec![[start.x * ends[3], start.y * ends[3], ends[3]]];
    let mut knots = vec![ends[2]];
    let sx = if values[2] == 0. { size.x } else { scale.x };
    let sy = if values[3] == 0. { size.y } else { scale.y };
    for p in values[4..].chunks_exact(4) {
        points.push([p[0] * sx * p[3], p[1] * sy * p[3], p[3]]);
        knots.push(p[2]);
    }
    points.push([end.x * ends[1], end.y * ends[1], ends[1]]);
    knots.push(ends[0]);
    if points.len() <= degree || points.len() > 1024 || points.iter().any(|p| p[2] <= 0.) {
        return None;
    }
    knots.resize(points.len() + degree + 1, values[0]);
    if knots.windows(2).any(|p| p[0] > p[1]) {
        return None;
    }
    let lo = knots[degree];
    let hi = knots[points.len()];
    if hi <= lo {
        return None;
    }
    let at = |t: f64| -> DVec2 {
        let t = t.clamp(lo, hi);
        let span = if t == hi {
            points.len() - 1
        } else {
            (degree..points.len())
                .find(|i| t < knots[i + 1])
                .unwrap_or(points.len() - 1)
        };
        let mut d = points[span - degree..=span].to_vec();
        for r in 1..=degree {
            for j in (r..=degree).rev() {
                let i = span - degree + j;
                let denominator = knots[i + degree + 1 - r] - knots[i];
                let alpha = if denominator.abs() < 1e-15 {
                    0.
                } else {
                    (t - knots[i]) / denominator
                };
                let previous = d[j - 1];
                for (value, previous) in d[j].iter_mut().zip(previous) {
                    *value = (1. - alpha) * previous + alpha * *value;
                }
            }
        }
        dvec2(d[degree][0], d[degree][1]) / d[degree][2]
    };
    let mut svg = String::new();
    let mut count = 0;
    fn span(
        out: &mut String,
        at: &impl Fn(f64) -> DVec2,
        a: f64,
        b: f64,
        depth: usize,
        count: &mut usize,
    ) -> Option<()> {
        let p = at(a);
        let q = at(b);
        let epsilon = (b - a) * 1e-5;
        let c = p + (at(a + epsilon) - p) * ((b - a) / (3. * epsilon));
        let d = q - (q - at(b - epsilon)) * ((b - a) / (3. * epsilon));
        let point = |t: f64| {
            p * (1. - t).powi(3)
                + c * (3. * (1. - t).powi(2) * t)
                + d * (3. * (1. - t) * t * t)
                + q * t.powi(3)
        };
        let error = [0.25, 0.5, 0.75]
            .into_iter()
            .map(|t| (point(t) - at(a + (b - a) * t)).length())
            .fold(0., f64::max);
        if error > 1e-5 && depth < 12 {
            let mid = (a + b) / 2.;
            span(out, at, a, mid, depth + 1, count)?;
            span(out, at, mid, b, depth + 1, count)?;
        } else {
            *count += 1;
            if *count > 4096 {
                return None;
            }
            if !c.is_finite() || !d.is_finite() || !q.is_finite() {
                return None;
            }
            write!(out, "C {} {} {} {} {} {} ", c.x, c.y, d.x, d.y, q.x, q.y).unwrap();
        }
        Some(())
    }
    for pair in knots[degree..=points.len()].windows(2) {
        if pair[1] > pair[0] {
            span(&mut svg, &at, pair[0], pair[1], 0, &mut count)?;
        }
    }
    Some(svg)
}

#[cfg(test)]
mod nurbs_tests {
    use super::*;
    #[test]
    fn rational_quarter_circle_is_smooth_and_keeps_radius() {
        let curve = nurbs(
            dvec2(1., 0.),
            dvec2(0., 1.),
            &[1., 2., 1., 1., 1., 1., 0., std::f64::consts::FRAC_1_SQRT_2],
            [0., 1., 0., 1.],
            DVec2::ONE,
            DVec2::ONE,
        )
        .unwrap();
        let path = emulsion_raster::vector::Path::from_svg(&format!("M 1 0 {curve}")).unwrap();
        for p in &path.flatten(0.0001)[0].0 {
            assert!((p.0.hypot(p.1) - 1.).abs() < 0.001);
        }
        assert!(path.subpaths[0].anchors.iter().any(|a| a.h_out != a.p));
    }
}
