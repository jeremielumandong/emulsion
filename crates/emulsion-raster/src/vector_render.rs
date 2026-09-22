use super::*;
use crate::color;
use rayon::prelude::*;
use tiny_skia::{FillRule, PathBuilder, Transform};

fn outline(path: &Path, closed: Option<bool>, ox: f64, oy: f64) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for sp in &path.subpaths {
        if closed.is_some_and(|c| c != sp.closed) {
            continue;
        }
        let Some(first) = sp.anchors.first() else {
            continue;
        };
        if sp.anchors.iter().any(|a| {
            [a.p, a.h_in, a.h_out]
                .iter()
                .any(|p| !p.0.is_finite() || !p.1.is_finite())
        }) {
            continue;
        }
        pb.move_to((first.p.0 - ox) as f32, (first.p.1 - oy) as f32);
        for i in 0..segment_count(sp) {
            let (a, b, c, d) = segment(sp, i).expect("counted segment");
            if a == b && c == d {
                pb.line_to((d.0 - ox) as f32, (d.1 - oy) as f32);
            } else {
                pb.cubic_to(
                    (b.0 - ox) as f32,
                    (b.1 - oy) as f32,
                    (c.0 - ox) as f32,
                    (c.1 - oy) as f32,
                    (d.0 - ox) as f32,
                    (d.1 - oy) as f32,
                );
            }
        }
        if sp.closed {
            pb.close();
        }
    }
    pb.finish()
}
fn coverage(path: Option<&tiny_skia::Path>, b: IRect) -> tiny_skia::Mask {
    let mut mask =
        tiny_skia::Mask::new(b.w as u32, b.h as u32).expect("non-empty bounded coverage");
    if let Some(path) = path {
        mask.fill_path(path, FillRule::Winding, true, Transform::identity());
    }
    mask
}
fn stroke(path: Option<tiny_skia::Path>, style: &PathStyle, width: f32) -> Option<tiny_skia::Path> {
    let mut path = path?;
    if style.dash_count > 0 {
        let mut intervals = style.dash[..style.dash_count as usize].to_vec();
        if intervals.len() % 2 == 1 {
            intervals.extend_from_within(..);
        }
        if let Some(dash) = tiny_skia::StrokeDash::new(intervals, style.dash_offset) {
            path = path.dash(&dash, 1.0)?;
        }
    }
    path.stroke(
        &tiny_skia::Stroke {
            width,
            miter_limit: style.miter_limit,
            line_cap: match style.cap {
                StrokeCap::Butt => tiny_skia::LineCap::Butt,
                StrokeCap::Round => tiny_skia::LineCap::Round,
                StrokeCap::Square => tiny_skia::LineCap::Square,
            },
            line_join: match style.join {
                StrokeJoin::Miter => tiny_skia::LineJoin::Miter,
                StrokeJoin::Round => tiny_skia::LineJoin::Round,
                StrokeJoin::Bevel => tiny_skia::LineJoin::Bevel,
            },
            dash: None,
        },
        1.0,
    )
}

/// Prepare colors and gradient direction once, not per document pixel.
#[derive(Clone, Copy)]
struct PaintSampler {
    primary: [f32; 4],
    secondary: [f32; 4],
    mode: PathPaint,
    direction: (f32, f32),
}
impl PaintSampler {
    fn new(primary: [u8; 4], mode: PathPaint) -> Self {
        let secondary = match mode {
            PathPaint::LinearGradient { end, .. } | PathPaint::RadialGradient { end } => end,
            PathPaint::Pattern { secondary, .. } => secondary,
            PathPaint::Solid => primary,
        };
        let direction = match mode {
            PathPaint::LinearGradient { angle, .. } => angle.to_radians().sin_cos(),
            _ => (0.0, 1.0),
        };
        Self {
            primary: color::srgba8_to_premul(primary),
            secondary: color::srgba8_to_premul(secondary),
            mode,
            direction,
        }
    }
    /// Paint coordinates follow the path, including when it is translated.
    fn at(&self, x: f32, y: f32, bounds: [f32; 4]) -> [f32; 4] {
        let [left, top, w, h] = bounds;
        let t = match self.mode {
            PathPaint::Solid => return self.primary,
            PathPaint::LinearGradient { .. } => {
                let (dy, dx) = self.direction;
                let span = (dx.abs() * w + dy.abs() * h).max(0.001);
                ((x - left - w / 2.0) * dx + (y - top - h / 2.0) * dy) / span + 0.5
            }
            PathPaint::RadialGradient { .. } => ((x - left - w / 2.0) / (w / 2.0).max(0.001))
                .hypot((y - top - h / 2.0) / (h / 2.0).max(0.001)),
            PathPaint::Pattern { kind, size, .. } => {
                let u = (x - left) / size;
                let v = (y - top) / size;
                let primary_cell = match kind {
                    PatternKind::Checker => {
                        (u.floor() as i64 + v.floor() as i64).rem_euclid(2) == 0
                    }
                    PatternKind::Stripes => u.floor() as i64 % 2 == 0,
                    PatternKind::Dots => {
                        (u.rem_euclid(1.0) - 0.5).hypot(v.rem_euclid(1.0) - 0.5) < 0.3
                    }
                };
                return if primary_cell {
                    self.primary
                } else {
                    self.secondary
                };
            }
        }
        .clamp(0.0, 1.0);
        std::array::from_fn(|i| self.primary[i] + (self.secondary[i] - self.primary[i]) * t)
    }
}

pub(super) fn rasterize(path: &Path, style: &PathStyle, w: u32, h: u32) -> Raster {
    let style = style.sanitized();
    let b = path
        .bounds(&style)
        .intersect(&IRect::new(0, 0, w as i32, h as i32));
    let empty = Raster::transparent(w, h);
    if b.is_empty() || path.is_empty() {
        return empty;
    }
    let shape = outline(path, None, b.x as f64, b.y as f64);
    let Some(shape) = shape else {
        return empty;
    };
    // tiny-skia's cached bounds include control points, not cubic extrema.
    // Paint spans the visible geometry so overshooting handles do not keep a
    // gradient from reaching its end color at the edge of the shape.
    let Some((x, y, width, height)) = crate::vector_geometry::bounds(path) else {
        return empty;
    };
    let paint_bounds = [x as f32, y as f32, width as f32, height as f32];
    let fill = style.fill.map(|_| coverage(Some(&shape), b));
    let stroke_mask = if style.stroke.is_some() && style.width > 0.0 {
        if style.alignment == StrokeAlignment::Center {
            Some(coverage(
                stroke(Some(shape.clone()), &style, style.width).as_ref(),
                b,
            ))
        } else {
            let closed = outline(path, Some(true), b.x as f64, b.y as f64);
            let interior = coverage(closed.as_ref(), b);
            let mut mask = coverage(stroke(closed, &style, style.width * 2.0).as_ref(), b);
            for (v, inside) in mask
                .data_mut()
                .iter_mut()
                .zip(interior.data().iter().copied())
            {
                let clip = if style.alignment == StrokeAlignment::Inside {
                    inside
                } else {
                    255 - inside
                };
                *v = ((*v as u16 * clip as u16 + 127) / 255) as u8;
            }
            // An open contour has no inside/outside; it retains a centered stroke.
            let open = coverage(
                stroke(
                    outline(path, Some(false), b.x as f64, b.y as f64),
                    &style,
                    style.width,
                )
                .as_ref(),
                b,
            );
            for (v, o) in mask.data_mut().iter_mut().zip(open.data().iter().copied()) {
                *v = (*v).max(o);
            }
            Some(mask)
        }
    } else {
        None
    };
    let fill_paint = style.fill.map(|c| PaintSampler::new(c, style.fill_paint));
    let stroke_paint = style
        .stroke
        .map(|c| PaintSampler::new(c, style.stroke_paint));
    let mut out = vec![[0; 4]; b.w as usize * b.h as usize];
    out.par_chunks_mut(b.w as usize)
        .enumerate()
        .for_each(|(row, pixels)| {
            for (col, pixel) in pixels.iter_mut().enumerate() {
                let index = row * b.w as usize + col;
                let x = b.x as f32 + col as f32 + 0.5;
                let y = b.y as f32 + row as f32 + 0.5;
                let mut acc = [0.0; 4];
                for (mask, paint) in [(&fill, fill_paint), (&stroke_mask, stroke_paint)] {
                    if let (Some(mask), Some(paint)) = (mask, paint) {
                        let k = mask.data()[index] as f32 / 255.0;
                        if k == 0.0 {
                            continue;
                        }
                        let c = paint.at(x, y, paint_bounds);
                        for i in 0..4 {
                            acc[i] = c[i] * k + acc[i] * (1.0 - c[3] * k);
                        }
                    }
                }
                *pixel = color::f_to_px(acc);
            }
        });
    empty.write_rect(b, &out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle() -> Path {
        Path::from_svg("M 20 20 L 60 20 L 60 60 L 20 60 Z").unwrap()
    }
    fn style() -> PathStyle {
        PathStyle {
            stroke: Some([255, 0, 0, 255]),
            width: 8.0,
            ..PathStyle::default()
        }
    }
    #[test]
    fn alignment_keeps_outline_on_requested_side() {
        let p = rectangle();
        let center = p.rasterize(&style(), 80, 80);
        assert!(center.get(18, 40)[3] > 60000 && center.get(22, 40)[3] > 60000);
        let inside = p.rasterize(
            &PathStyle {
                alignment: StrokeAlignment::Inside,
                ..style()
            },
            80,
            80,
        );
        assert_eq!(inside.get(18, 40)[3], 0);
        assert!(inside.get(26, 40)[3] > 60000);
        let outside = p.rasterize(
            &PathStyle {
                alignment: StrokeAlignment::Outside,
                ..style()
            },
            80,
            80,
        );
        assert_eq!(outside.get(22, 40)[3], 0);
        assert!(outside.get(13, 40)[3] > 60000);
    }
    #[test]
    fn cap_join_and_dash_are_visible_geometry() {
        let p = Path::from_svg("M 20 40 L 60 40").unwrap();
        let butt = p.rasterize(
            &PathStyle {
                cap: StrokeCap::Butt,
                ..style()
            },
            80,
            80,
        );
        let round = p.rasterize(&style(), 80, 80);
        let square = p.rasterize(
            &PathStyle {
                cap: StrokeCap::Square,
                ..style()
            },
            80,
            80,
        );
        assert_eq!(butt.get(17, 40)[3], 0);
        assert!(round.get(17, 40)[3] > 0);
        assert_eq!(round.get(16, 36)[3], 0);
        assert!(square.get(16, 36)[3] > 60000);
        let dash = p.rasterize(
            &PathStyle {
                cap: StrokeCap::Butt,
                dash: [8.0, 8.0, 0.0, 0.0, 0.0, 0.0],
                dash_count: 2,
                ..style()
            },
            80,
            80,
        );
        assert!(dash.get(24, 40)[3] > 60000);
        assert_eq!(dash.get(32, 40)[3], 0);
        let corner = Path::from_svg("M 20 60 L 20 20 L 60 20").unwrap();
        let miter = corner.rasterize(
            &PathStyle {
                join: StrokeJoin::Miter,
                ..style()
            },
            80,
            80,
        );
        let bevel = corner.rasterize(
            &PathStyle {
                join: StrokeJoin::Bevel,
                ..style()
            },
            80,
            80,
        );
        assert!(miter.get(16, 16)[3] > 60000);
        assert_eq!(bevel.get(16, 16)[3], 0);
    }
    #[test]
    fn dash_offset_open_alignment_and_holes_are_respected() {
        let line = Path::from_svg("M 20 40 L 60 40").unwrap();
        let s = PathStyle {
            cap: StrokeCap::Butt,
            dash: [8.0, 8.0, 0.0, 0.0, 0.0, 0.0],
            dash_count: 2,
            dash_offset: 8.0,
            alignment: StrokeAlignment::Inside,
            ..style()
        };
        let result = line.rasterize(&s, 80, 80);
        assert_eq!(result.get(24, 40)[3], 0);
        assert!(result.get(32, 38)[3] > 60000);
        assert!(result.get(32, 42)[3] > 60000);
        let ring =
            Path::from_svg("M 10 10 L 70 10 L 70 70 L 10 70 Z M 30 30 L 30 50 L 50 50 L 50 30 Z")
                .unwrap();
        let result = ring.rasterize(
            &PathStyle {
                stroke: None,
                fill: Some([255, 0, 0, 255]),
                ..style()
            },
            80,
            80,
        );
        assert!(result.get(20, 40)[3] > 60000);
        assert_eq!(result.get(40, 40)[3], 0);
    }
    #[test]
    fn zero_length_round_dashes_produce_dots_with_clear_gaps() {
        let line = Path::from_svg("M 20 40 L 70 40").unwrap();
        let s = PathStyle {
            dash: [0.0, 16.0, 0.0, 0.0, 0.0, 0.0],
            dash_count: 2,
            cap: StrokeCap::Round,
            ..style()
        };
        let result = line.rasterize(&s, 90, 80);
        for x in [20, 36, 52, 68] {
            assert!(result.get(x, 40)[3] > 60000, "dot at {x}");
        }
        for x in [28, 44, 60] {
            assert_eq!(result.get(x, 40)[3], 0, "gap at {x}");
        }
        assert_eq!(result.get(16, 36)[3], 0, "dot is round, not square");
    }
    #[test]
    fn transparent_paints_never_leak_hidden_rgb_into_edges() {
        let s = PathStyle {
            stroke: None,
            fill: Some([255, 0, 0, 255]),
            fill_paint: PathPaint::LinearGradient {
                end: [0, 255, 0, 0],
                angle: 0.0,
            },
            ..style()
        };
        let result = rectangle().rasterize(&s, 80, 80);
        let edge = result.get(58, 40);
        assert!(edge[3] > 0 && edge[3] < 65535);
        assert_eq!(
            edge[0], edge[3],
            "red stays premultiplied at translucent edge"
        );
        assert_eq!(edge[1], 0, "hidden green must not contaminate the gradient");
        let pattern = rectangle().rasterize(
            &PathStyle {
                fill_paint: PathPaint::Pattern {
                    kind: PatternKind::Checker,
                    secondary: [0, 255, 0, 0],
                    size: 8.0,
                },
                ..s
            },
            80,
            80,
        );
        assert_eq!(pattern.get(30, 22), [0; 4]);
        assert_eq!(pattern.get(22, 22), [65535, 0, 0, 65535]);
    }
    #[test]
    fn curved_gradient_spans_geometry_instead_of_overshooting_handles() {
        // Handles reach x=100, but the curve's rightmost point is x=80.
        let path = Path::from_svg("M 20 20 C 100 20 100 60 20 60 Z").unwrap();
        let result = path.rasterize(
            &PathStyle {
                stroke: None,
                fill: Some([255, 0, 0, 255]),
                fill_paint: PathPaint::LinearGradient {
                    end: [0, 0, 255, 255],
                    angle: 0.0,
                },
                ..style()
            },
            120,
            80,
        );
        let edge = color::px_to_f(result.get(77, 40));
        assert!(
            edge[3] > 0.99 && edge[2] > 0.94 && edge[0] < 0.06,
            "end color at geometric edge: {edge:?}"
        );
    }
    #[test]
    fn subpixel_dash_lengths_are_bounded_and_small_shapes_stay_sparse() {
        let s = PathStyle {
            dash: [f32::MIN_POSITIVE, 0.0, 0.0, 0.0, 0.0, 0.0],
            dash_count: 2,
            ..style()
        }
        .sanitized();
        assert_eq!(s.dash[0], 0.25);
        assert_eq!(s.dash[1], 0.0, "zero remains a valid dot or gap interval");
        let result = rectangle().rasterize(
            &PathStyle {
                stroke: None,
                fill: Some([255; 4]),
                ..style()
            },
            4096,
            4096,
        );
        assert_eq!(
            result.tile_count(),
            1,
            "document size must not allocate full canvas tiles for a small shape"
        );
        assert_eq!(result.get(4095, 4095), [0; 4]);
    }
    #[test]
    fn gradients_patterns_and_alpha_render_natively() {
        let p = rectangle();
        let base = PathStyle {
            stroke: None,
            fill: Some([255, 0, 0, 255]),
            ..PathStyle::default()
        };
        let linear = p.rasterize(
            &PathStyle {
                fill_paint: PathPaint::LinearGradient {
                    end: [0, 0, 255, 255],
                    angle: 0.0,
                },
                ..base
            },
            80,
            80,
        );
        assert!(linear.get(22, 40)[0] > linear.get(22, 40)[2]);
        assert!(linear.get(58, 40)[2] > linear.get(58, 40)[0]);
        let radial = p.rasterize(
            &PathStyle {
                fill_paint: PathPaint::RadialGradient {
                    end: [0, 0, 255, 0],
                },
                ..base
            },
            80,
            80,
        );
        assert!(radial.get(40, 40)[3] > radial.get(58, 40)[3]);
        for kind in [
            PatternKind::Checker,
            PatternKind::Stripes,
            PatternKind::Dots,
        ] {
            let patterned = p.rasterize(
                &PathStyle {
                    fill_paint: PathPaint::Pattern {
                        kind,
                        secondary: [0, 255, 0, 255],
                        size: 8.0,
                    },
                    ..base
                },
                80,
                80,
            );
            let data = patterned.read_rect(IRect::new(20, 20, 40, 40));
            assert!(data.iter().any(|c| c[0] > 60000));
            assert!(data.iter().any(|c| c[1] > 60000));
        }
    }
    #[test]
    fn invalid_style_values_are_sanitized_and_translation_preserves_paint() {
        let s = PathStyle {
            width: f32::NAN,
            miter_limit: f32::INFINITY,
            dash_count: 255,
            dash: [f32::NAN; 6],
            dash_offset: f32::NAN,
            fill_paint: PathPaint::Pattern {
                kind: PatternKind::Dots,
                secondary: [0; 4],
                size: 0.0,
            },
            ..style()
        }
        .sanitized();
        assert_eq!(s.width, 3.0);
        assert_eq!(s.miter_limit, 4.0);
        assert_eq!(s.dash_count, 6);
        assert!(s.dash.iter().all(|x| x.is_finite()));
        let mut p = rectangle();
        let s = PathStyle {
            stroke: None,
            fill: Some([255, 0, 0, 255]),
            fill_paint: PathPaint::LinearGradient {
                end: [0, 0, 255, 255],
                angle: 45.0,
            },
            ..style()
        };
        let before = p.rasterize(&s, 100, 100);
        p.translate(10.0, 10.0);
        let after = p.rasterize(&s, 100, 100);
        assert_eq!(before.get(35, 35), after.get(45, 45));
    }
}
