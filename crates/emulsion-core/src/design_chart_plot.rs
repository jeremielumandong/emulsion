//! Native Cartesian chart geometry, clipped geometrically to the plot rectangle.
use super::*;
type Point = (f64, f64);
fn polygon(points: Vec<Point>) -> Path {
    Path {
        subpaths: vec![SubPath {
            anchors: points.into_iter().map(Anchor::corner).collect(),
            closed: true,
        }],
    }
}
fn shape(name: &str, path: Path, color: [u8; 4], canvas: (u32, u32)) -> Node {
    Node::path(
        0,
        name,
        Arc::new(path),
        PathStyle {
            fill: Some(color),
            stroke: None,
            ..Default::default()
        },
        canvas.0,
        canvas.1,
    )
}
fn text(
    value: String,
    x: f64,
    y: f64,
    width: f64,
    size: f32,
    color: [u8; 4],
    canvas: (u32, u32),
) -> Node {
    Node::text(
        0,
        "Chart label",
        crate::text::TextSpec {
            text: value,
            x: x as f32,
            y: y as f32,
            width: Some(width.max(1.) as f32),
            height: Some(size * 2.),
            size,
            color,
            font: "Geist".into(),
            ..Default::default()
        },
        canvas.0,
        canvas.1,
    )
}
fn tick(v: f64) -> String {
    if v == 0. {
        "0".into()
    } else if v.abs() >= 1e6 || v.abs() < 0.001 {
        format!("{v:.2e}")
    } else {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').into()
    }
}
fn segment(a: Point, b: Point, r: [f64; 4]) -> Option<(Point, Point)> {
    let d = (b.0 - a.0, b.1 - a.1);
    let (mut lo, mut hi) = (0_f64, 1_f64);
    for (p, q) in [
        (-d.0, a.0 - r[0]),
        (d.0, r[2] - a.0),
        (-d.1, a.1 - r[1]),
        (d.1, r[3] - a.1),
    ] {
        if p.abs() < 1e-12 {
            if q < 0. {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0. {
                lo = lo.max(t)
            } else {
                hi = hi.min(t)
            }
        }
    }
    (lo <= hi).then_some((
        (a.0 + lo * d.0, a.1 + lo * d.1),
        (a.0 + hi * d.0, a.1 + hi * d.1),
    ))
}
fn clip_polygon(mut p: Vec<Point>, r: [f64; 4]) -> Vec<Point> {
    for edge in 0..4 {
        let inside = |p: Point| match edge {
            0 => p.0 >= r[0],
            1 => p.0 <= r[2],
            2 => p.1 >= r[1],
            _ => p.1 <= r[3],
        };
        let intersect = |a: Point, b: Point| {
            if edge < 2 {
                let x = if edge == 0 { r[0] } else { r[2] };
                let t = (x - a.0) / (b.0 - a.0);
                (x, a.1 + t * (b.1 - a.1))
            } else {
                let y = if edge == 2 { r[1] } else { r[3] };
                let t = (y - a.1) / (b.1 - a.1);
                (a.0 + t * (b.0 - a.0), y)
            }
        };
        if p.is_empty() {
            break;
        }
        let mut out = Vec::new();
        let mut a = *p.last().unwrap();
        for b in p {
            if inside(a) != inside(b) {
                out.push(intersect(a, b));
            }
            if inside(b) {
                out.push(b);
            }
            a = b;
        }
        p = out;
    }
    p
}
pub(super) fn draw(c: &Chart, origin: Point, canvas: (u32, u32)) -> Result<Vec<Node>, String> {
    let (x, y) = origin;
    let (w, h) = c.size;
    let ink = [32, 38, 48, 255];
    let grid = [218, 222, 230, 255];
    let values: Vec<Vec<f64>> = c.rows[1..]
        .iter()
        .map(|r| r[1..].iter().map(|v| v.trim().parse().unwrap()).collect())
        .collect();
    let natural = if c.kind == Kind::StackedBar {
        values.iter().fold((0_f64, 0_f64), |(lo, hi), r| {
            (
                lo.min(r.iter().filter(|v| **v < 0.).sum()),
                hi.max(r.iter().filter(|v| **v > 0.).sum()),
            )
        })
    } else {
        values
            .iter()
            .flatten()
            .fold((0_f64, 0_f64), |(lo, hi), v| (lo.min(*v), hi.max(*v)))
    };
    let (min, max) = c.y_axis.range(natural)?;
    let xs: Vec<f64> = if c.kind == Kind::Scatter {
        c.rows[1..]
            .iter()
            .map(|r| r[0].trim().parse().unwrap())
            .collect()
    } else {
        Vec::new()
    };
    let (xmin, xmax) = if xs.is_empty() {
        (0., 1.)
    } else {
        c.x_axis.range(
            xs.iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(*v), hi.max(*v))
                }),
        )?
    };
    let (left, top, pw, ph) = (x + 58., y + 66., w - 80., (h - 150.).max(10.));
    let rect = [left, top, left + pw, top + ph];
    let py = |v: f64| top + (max - v) / (max - min) * ph;
    let px = |v: f64| left + (v - xmin) / (xmax - xmin) * pw;
    let zero = py(0_f64.clamp(min, max));
    let mut out = Vec::new();
    for i in 0..c.y_axis.ticks {
        let v = min + (max - min) * i as f64 / (c.y_axis.ticks - 1) as f64;
        let yy = py(v);
        out.push(shape(
            "Axis grid",
            rectangle(left, yy, pw, 0.6),
            grid,
            canvas,
        ));
        if c.y_axis.show_labels {
            out.push(text(tick(v), x + 2., yy - 5., 52., 10., ink, canvas));
        }
    }
    if !c.y_axis.label.is_empty() {
        out.push(text(
            c.y_axis.label.clone(),
            left,
            y + 43.,
            pw,
            11.,
            ink,
            canvas,
        ));
    }
    if !c.x_axis.label.is_empty() {
        out.push(text(
            c.x_axis.label.clone(),
            left,
            y + h - 47.,
            pw,
            11.,
            ink,
            canvas,
        ));
    }
    if c.kind == Kind::Scatter && c.x_axis.show_labels {
        for i in 0..c.x_axis.ticks {
            let v = xmin + (xmax - xmin) * i as f64 / (c.x_axis.ticks - 1) as f64;
            out.push(text(
                tick(v),
                px(v) - 15.,
                top + ph + 5.,
                50.,
                10.,
                ink,
                canvas,
            ));
        }
    }
    let step = pw / values.len() as f64;
    let series = c.rows[0].len() - 1;
    let mut positive = vec![0.; values.len()];
    let mut negative = positive.clone();
    for s in 0..series {
        let color = c.colors[s % c.colors.len()];
        out.push(text(
            c.rows[0][s + 1].clone(),
            left + s as f64 * pw / series as f64,
            y + h - 24.,
            pw / series as f64,
            10.,
            color,
            canvas,
        ));
        let mut points = Vec::new();
        for (i, row) in values.iter().enumerate() {
            let xx = if c.kind == Kind::Scatter {
                px(xs[i])
            } else {
                left + (i as f64 + 0.5) * step
            };
            let yy = py(row[s]);
            match c.kind {
                Kind::Bar | Kind::StackedBar => {
                    let (base, value, bx, bw) = if c.kind == Kind::StackedBar {
                        let b = if row[s] >= 0. {
                            &mut positive[i]
                        } else {
                            &mut negative[i]
                        };
                        let from = *b;
                        *b += row[s];
                        (from, *b, left + i as f64 * step + step * 0.15, step * 0.7)
                    } else {
                        (
                            0.,
                            row[s],
                            left + i as f64 * step
                                + step * 0.1
                                + s as f64 * step * 0.8 / series as f64,
                            step * 0.72 / series as f64,
                        )
                    };
                    let a = py(base).clamp(top, top + ph);
                    let b = py(value).clamp(top, top + ph);
                    if (a - b).abs() > 1e-9 {
                        out.push(shape(
                            "Bar",
                            rectangle(bx, a.min(b), bw, (a - b).abs()),
                            color,
                            canvas,
                        ));
                    }
                }
                Kind::Scatter => {
                    if xs[i] >= xmin && xs[i] <= xmax && row[s] >= min && row[s] <= max {
                        let p = clip_polygon(
                            (0..24)
                                .map(|i| {
                                    let a = i as f64 / 24. * std::f64::consts::TAU;
                                    (xx + 3. * a.cos(), yy + 3. * a.sin())
                                })
                                .collect(),
                            rect,
                        );
                        out.push(shape("Scatter point", polygon(p), color, canvas));
                    }
                }
                _ => {
                    points.push((xx, yy));
                }
            }
            if s == 0 && c.kind != Kind::Scatter && c.x_axis.show_labels {
                out.push(text(
                    c.rows[i + 1][0].clone(),
                    left + i as f64 * step,
                    top + ph + 5.,
                    step,
                    10.,
                    ink,
                    canvas,
                ));
            }
        }
        if c.kind == Kind::Area && !points.is_empty() {
            let mut area = vec![(points[0].0, zero)];
            area.extend(points.iter().copied());
            area.push((points.last().unwrap().0, zero));
            let area = clip_polygon(area, rect);
            if area.len() >= 3 {
                out.push(shape("Series area", polygon(area), color, canvas));
            }
        }
        if matches!(c.kind, Kind::Line | Kind::Area) {
            for &(xx, yy) in &points {
                if yy >= top && yy <= top + ph {
                    let marker = clip_polygon(
                        (0..24)
                            .map(|i| {
                                let a = i as f64 / 24. * std::f64::consts::TAU;
                                (xx + 3. * a.cos(), yy + 3. * a.sin())
                            })
                            .collect(),
                        rect,
                    );
                    out.push(shape("Series point", polygon(marker), color, canvas));
                }
            }
            let subpaths = points
                .windows(2)
                .filter_map(|p| segment(p[0], p[1], rect))
                .map(|(a, b)| SubPath {
                    anchors: vec![Anchor::corner(a), Anchor::corner(b)],
                    closed: false,
                })
                .collect::<Vec<_>>();
            if !subpaths.is_empty() {
                out.push(Node::path(
                    0,
                    "Series line",
                    Arc::new(Path { subpaths }),
                    PathStyle {
                        fill: None,
                        stroke: Some(color),
                        width: 2.,
                        ..Default::default()
                    },
                    canvas.0,
                    canvas.1,
                ));
            }
        }
    }
    Ok(out)
}
