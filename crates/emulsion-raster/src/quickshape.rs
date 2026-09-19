//! QuickShape: fit a hand-drawn stroke to the shape it was aiming for. A
//! wobbly line becomes straight, a rough loop a circle or ellipse, a
//! lumpy triangle a triangle. The fit is geometric and cheap; the caller
//! decides when to invoke it (Procreate snaps when the pen is held still
//! at the end of the stroke).

/// A fitted shape, in the same pixel space as the input points.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Line((f32, f32), (f32, f32)),
    /// Straight segments through these vertices, open.
    Polyline(Vec<(f32, f32)>),
    /// Straight segments through these vertices, closed.
    Polygon(Vec<(f32, f32)>),
    Circle {
        center: (f32, f32),
        radius: f32,
    },
    Ellipse {
        center: (f32, f32),
        /// Semi-axes along `angle` and perpendicular to it.
        radii: (f32, f32),
        /// Radians, of the first axis.
        angle: f32,
    },
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Distance from `p` to the segment `a`–`b`.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-9 {
        return dist(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    dist(p, (a.0 + dx * t, a.1 + dy * t))
}

/// Ramer–Douglas–Peucker simplification; keeps the first and last point.
pub fn simplify(pts: &[(f32, f32)], eps: f32) -> Vec<(f32, f32)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (mut worst, mut at) = (0.0f32, 0usize);
    for (i, p) in pts.iter().enumerate().skip(1).take(pts.len() - 2) {
        let d = seg_dist(*p, a, b);
        if d > worst {
            worst = d;
            at = i;
        }
    }
    if worst <= eps {
        return vec![a, b];
    }
    let mut left = simplify(&pts[..=at], eps);
    let right = simplify(&pts[at..], eps);
    left.pop();
    left.extend(right);
    left
}

/// Resample a polyline to points `step` apart along its length, so a slow
/// (dense) part of the stroke does not outweigh a fast one.
fn resample(pts: &[(f32, f32)], step: f32) -> Vec<(f32, f32)> {
    let mut out = vec![pts[0]];
    let mut carry = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = dist(a, b);
        if len < 1e-6 {
            continue;
        }
        let mut d = step - carry;
        while d <= len {
            let f = d / len;
            out.push((a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f));
            d += step;
        }
        carry = len - (d - step);
    }
    let end = pts[pts.len() - 1];
    if out.len() < 2 || dist(*out.last().unwrap(), end) > step * 0.25 {
        out.push(end);
    }
    out
}

/// Mean distance from the samples to the closed polygon `verts`.
fn polygon_dev(pts: &[(f32, f32)], verts: &[(f32, f32)]) -> f32 {
    let n = verts.len();
    pts.iter()
        .map(|p| {
            (0..n)
                .map(|i| seg_dist(*p, verts[i], verts[(i + 1) % n]))
                .fold(f32::MAX, f32::min)
        })
        .sum::<f32>()
        / pts.len() as f32
}

/// Least-squares conic fit specialised to an ellipse: principal axes from
/// the covariance of arc-length-uniform samples, radii from the mean
/// absolute projection. Returns (center, radii, angle).
fn fit_ellipse(pts: &[(f32, f32)]) -> ((f32, f32), (f32, f32), f32) {
    let n = pts.len() as f32;
    let cx = pts.iter().map(|p| p.0).sum::<f32>() / n;
    let cy = pts.iter().map(|p| p.1).sum::<f32>() / n;
    let (mut sxx, mut sxy, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for p in pts {
        let (dx, dy) = (p.0 - cx, p.1 - cy);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (ca, sa) = (angle.cos(), angle.sin());
    let (mut ma, mut mb) = (0.0f32, 0.0f32);
    for p in pts {
        let (dx, dy) = (p.0 - cx, p.1 - cy);
        ma += (dx * ca + dy * sa).abs();
        mb += (-dx * sa + dy * ca).abs();
    }
    // For a uniformly sampled ellipse, mean |projection| ≈ 2r/π per axis;
    // good enough to seed a least-squares solve of u²/a² + v²/b² = 1,
    // which is linear in (1/a², 1/b²) and unbiased by where the hand
    // lingered.
    let k = std::f32::consts::FRAC_PI_2 / n;
    let seed = (ma * k, mb * k);
    let (mut s11, mut s12, mut s22, mut t1, mut t2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for p in pts {
        let (dx, dy) = (p.0 - cx, p.1 - cy);
        let (u, v) = ((dx * ca + dy * sa) as f64, (-dx * sa + dy * ca) as f64);
        let (u2, v2) = (u * u, v * v);
        s11 += u2 * u2;
        s12 += u2 * v2;
        s22 += v2 * v2;
        t1 += u2;
        t2 += v2;
    }
    let det = s11 * s22 - s12 * s12;
    let radii = if det.abs() > 1e-9 {
        let ia = (t1 * s22 - t2 * s12) / det;
        let ib = (s11 * t2 - s12 * t1) / det;
        if ia > 0.0 && ib > 0.0 {
            ((1.0 / ia.sqrt()) as f32, (1.0 / ib.sqrt()) as f32)
        } else {
            seed
        }
    } else {
        seed
    };
    ((cx, cy), radii, angle)
}

/// Mean distance (pixels, radial) of the points from the ellipse outline.
fn ellipse_dev(pts: &[(f32, f32)], c: (f32, f32), r: (f32, f32), angle: f32) -> f32 {
    let (ca, sa) = (angle.cos(), angle.sin());
    let mean_r = (r.0 + r.1) * 0.5;
    let mut sum = 0.0;
    for p in pts {
        let (dx, dy) = (p.0 - c.0, p.1 - c.1);
        let (u, v) = ((dx * ca + dy * sa) / r.0, (-dx * sa + dy * ca) / r.1);
        let rho = (u * u + v * v).sqrt();
        sum += ((rho - 1.0) * mean_r).abs();
    }
    sum / pts.len() as f32
}

/// Fit `pts` (raw stroke input, in order). `None` when the stroke does not
/// resemble any shape closely enough to snap.
pub fn fit(pts: &[(f32, f32)]) -> Option<Shape> {
    if pts.len() < 2 {
        return None;
    }
    let (minx, maxx) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p.0), hi.max(p.0))
    });
    let (miny, maxy) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p.1), hi.max(p.1))
    });
    let diag = (maxx - minx).hypot(maxy - miny);
    if diag < 6.0 {
        return None;
    }
    let pts = resample(pts, (diag / 200.0).max(0.5));
    let length: f32 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let chord = dist(a, b);

    // A line: everything near the chord, and the chord nearly as long as
    // the path (rules out a there-and-back).
    let worst = pts
        .iter()
        .map(|p| seg_dist(*p, a, b))
        .fold(0.0f32, f32::max);
    if worst <= (chord * 0.05).max(2.5) && chord > length * 0.85 {
        return Some(Shape::Line(a, b));
    }

    let closed = chord < length * 0.22 && length > diag * 2.2;
    let eps = (diag * 0.05).max(2.0);
    let mut verts = simplify(&pts, eps);
    if closed {
        // The seam is arbitrary: join the two ends into one vertex.
        if verts.len() >= 3 {
            let last = verts.pop().unwrap();
            let first = verts[0];
            verts[0] = ((first.0 + last.0) * 0.5, (first.1 + last.1) * 0.5);
        }
        // Drop vertices that barely turn: a shape's corners are sharp.
        let n = verts.len();
        let mut keep = Vec::with_capacity(n);
        for i in 0..n {
            let (p, c, q) = (verts[(i + n - 1) % n], verts[i], verts[(i + 1) % n]);
            let (ux, uy) = (c.0 - p.0, c.1 - p.1);
            let (vx, vy) = (q.0 - c.0, q.1 - c.1);
            let dot = ux * vx + uy * vy;
            let cross = (ux * vy - uy * vx).abs();
            let turn = cross.atan2(dot);
            if turn > 25f32.to_radians() {
                keep.push(c);
            }
        }
        // A polygon and an ellipse both explain a loop; take whichever the
        // samples hug more tightly. A hexagon simplified out of a circle
        // strays from its edges by the simplification tolerance, a drawn
        // hexagon only by hand wobble.
        let poly = (3..=6)
            .contains(&keep.len())
            .then(|| polygon_dev(&pts, &keep));
        let (c, r, angle) = fit_ellipse(&pts);
        let ell = (r.0 >= 1.0 && r.1 >= 1.0).then(|| ellipse_dev(&pts, c, r, angle));
        let mean_r = (r.0 + r.1) * 0.5;
        return match (poly, ell) {
            (Some(pd), Some(ed)) if pd < ed * 0.8 && pd <= eps => Some(Shape::Polygon(keep)),
            (_, Some(ed)) if ed <= mean_r * 0.14 => {
                let (big, small) = (r.0.max(r.1), r.0.min(r.1));
                if (big - small) / big < 0.12 {
                    Some(Shape::Circle {
                        center: c,
                        radius: (big + small) * 0.5,
                    })
                } else {
                    Some(Shape::Ellipse {
                        center: c,
                        radii: r,
                        angle,
                    })
                }
            }
            (Some(pd), _) if pd <= eps => Some(Shape::Polygon(keep)),
            _ => None,
        };
    }

    // Open: a few straight segments (an L, a zigzag, a V).
    if (3..=6).contains(&verts.len()) {
        // Only when the corners are real turns, not a gentle curve.
        let sharp = verts.windows(3).all(|w| {
            let (ux, uy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let (vx, vy) = (w[2].0 - w[1].0, w[2].1 - w[1].1);
            (ux * vy - uy * vx).abs().atan2(ux * vx + uy * vy) > 30f32.to_radians()
        });
        if sharp {
            return Some(Shape::Polyline(verts));
        }
    }
    None
}

impl Shape {
    /// Points along the shape about `step` apart, starting near `from`
    /// (the stroke's first point) so the brush's own dynamics read the
    /// same way the hand drew it. Closed shapes come back to their start.
    pub fn outline(&self, step: f32, from: (f32, f32)) -> Vec<(f32, f32)> {
        let step = step.max(0.25);
        match self {
            Shape::Line(a, b) => resample(&[*a, *b], step),
            Shape::Polyline(v) => resample(v, step),
            Shape::Polygon(v) => {
                let mut v = rotate_to(v, from);
                v.push(v[0]);
                resample(&v, step)
            }
            Shape::Circle { center, radius } => Shape::Ellipse {
                center: *center,
                radii: (*radius, *radius),
                angle: 0.0,
            }
            .outline(step, from),
            Shape::Ellipse {
                center,
                radii,
                angle,
            } => {
                let (ca, sa) = (angle.cos(), angle.sin());
                let at = |t: f32| {
                    let (x, y) = (radii.0 * t.cos(), radii.1 * t.sin());
                    (center.0 + x * ca - y * sa, center.1 + x * sa + y * ca)
                };
                // Start at the parameter nearest the hand's first point.
                let (dx, dy) = (from.0 - center.0, from.1 - center.1);
                let (u, v) = (
                    (dx * ca + dy * sa) / radii.0,
                    (-dx * sa + dy * ca) / radii.1,
                );
                let t0 = v.atan2(u);
                let perim = std::f32::consts::PI
                    * (3.0 * (radii.0 + radii.1)
                        - ((3.0 * radii.0 + radii.1) * (radii.0 + 3.0 * radii.1)).sqrt());
                let n = ((perim / step).ceil() as usize).clamp(16, 4096);
                let dense: Vec<(f32, f32)> = (0..=n)
                    .map(|i| at(t0 + i as f32 / n as f32 * std::f32::consts::TAU))
                    .collect();
                resample(&dense, step)
            }
        }
    }
}

/// The polygon's vertex order, starting at the vertex nearest `from`.
fn rotate_to(v: &[(f32, f32)], from: (f32, f32)) -> Vec<(f32, f32)> {
    let start = (0..v.len())
        .min_by(|&i, &j| dist(v[i], from).total_cmp(&dist(v[j], from)))
        .unwrap_or(0);
    let mut out = v[start..].to_vec();
    out.extend_from_slice(&v[..start]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jitter(i: usize) -> f32 {
        // Deterministic ±1.5 px wobble.
        ((i as f32 * 12.9898).sin() * 43758.547).fract() * 3.0 - 1.5
    }

    #[test]
    fn wobbly_line_snaps_to_a_line() {
        let pts: Vec<(f32, f32)> = (0..80)
            .map(|i| (10.0 + i as f32 * 3.0, 20.0 + i as f32 * 1.5 + jitter(i)))
            .collect();
        match fit(&pts) {
            Some(Shape::Line(a, b)) => {
                assert!(dist(a, pts[0]) < 0.01 && dist(b, pts[79]) < 0.01);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rough_loop_becomes_a_circle_and_a_squashed_one_an_ellipse() {
        let circle: Vec<(f32, f32)> = (0..120)
            .map(|i| {
                let t = i as f32 / 118.0 * std::f32::consts::TAU;
                (
                    100.0 + 50.0 * t.cos() + jitter(i),
                    80.0 + 50.0 * t.sin() + jitter(i + 7),
                )
            })
            .collect();
        match fit(&circle) {
            Some(Shape::Circle { center, radius }) => {
                assert!(dist(center, (100.0, 80.0)) < 2.0, "{center:?}");
                assert!((radius - 50.0).abs() < 2.5, "{radius}");
            }
            other => panic!("{other:?}"),
        }
        let ell: Vec<(f32, f32)> = (0..150)
            .map(|i| {
                let t = i as f32 / 148.0 * std::f32::consts::TAU;
                let (x, y) = (90.0 * t.cos(), 40.0 * t.sin());
                let (c, s) = (0.5f32.cos(), 0.5f32.sin());
                (200.0 + x * c - y * s + jitter(i), 200.0 + x * s + y * c)
            })
            .collect();
        match fit(&ell) {
            Some(Shape::Ellipse {
                center,
                radii,
                angle,
            }) => {
                assert!(dist(center, (200.0, 200.0)) < 2.0);
                assert!(
                    (radii.0 - 90.0).abs() < 5.0 && (radii.1 - 40.0).abs() < 4.0,
                    "{radii:?}"
                );
                assert!((angle - 0.5).abs() < 0.05, "{angle}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn triangle_and_open_corner_keep_their_vertices() {
        let corners = [(10.0, 10.0), (110.0, 20.0), (50.0, 100.0)];
        let mut pts = Vec::new();
        for i in 0..3 {
            let (a, b) = (corners[i], corners[(i + 1) % 3]);
            for k in 0..40 {
                let f = k as f32 / 40.0;
                pts.push((a.0 + (b.0 - a.0) * f + jitter(k), a.1 + (b.1 - a.1) * f));
            }
        }
        pts.push((11.0, 11.0));
        match fit(&pts) {
            Some(Shape::Polygon(v)) => {
                assert_eq!(v.len(), 3, "{v:?}");
                for c in corners {
                    assert!(v.iter().any(|p| dist(*p, c) < 4.0), "{v:?}");
                }
            }
            other => panic!("{other:?}"),
        }
        let mut l = Vec::new();
        for k in 0..50 {
            l.push((10.0, 10.0 + k as f32 * 2.0 + jitter(k)));
        }
        for k in 0..50 {
            l.push((10.0 + k as f32 * 2.0, 110.0 + jitter(k)));
        }
        match fit(&l) {
            Some(Shape::Polyline(v)) => assert_eq!(v.len(), 3, "{v:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_scribble_is_left_alone_and_outlines_close() {
        let pts: Vec<(f32, f32)> = (0..200)
            .map(|i| {
                let t = i as f32 * 0.3;
                (
                    100.0 + t * 2.0 + 30.0 * (t * 1.7).sin(),
                    100.0 + 40.0 * (t * 2.3).cos(),
                )
            })
            .collect();
        assert_eq!(fit(&pts), None);
        let o = Shape::Circle {
            center: (0.0, 0.0),
            radius: 10.0,
        }
        .outline(1.0, (10.0, 0.0));
        assert!(dist(o[0], (10.0, 0.0)) < 0.5, "{:?}", o[0]);
        assert!(dist(*o.last().unwrap(), (10.0, 0.0)) < 1.5);
        assert!(o.len() > 55 && o.len() < 70, "{}", o.len());
        let sq = Shape::Polygon(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)])
            .outline(1.0, (9.0, 9.0));
        assert!(dist(sq[0], (10.0, 10.0)) < 0.01);
        assert!(dist(*sq.last().unwrap(), (10.0, 10.0)) < 1.01);
    }
}
