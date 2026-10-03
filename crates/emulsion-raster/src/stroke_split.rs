//! Splitting a vector stroke layer by a selection, for the cutter: strokes
//! are cut where they cross the selection's edge, and fills are clipped to
//! the selection's traced outline, so both halves stay vector.

use crate::gap_fill::trace;
use crate::image::Mask;
use crate::strokes::{Stroke, StrokeFill, StrokePoint, StrokeSet};
use crate::vector::{Anchor, Path, Pt, SubPath};
use crate::vector_geometry::{BooleanOp, boolean};

/// Pixels between the samples that look for a crossing along a segment.
const SAMPLE_PX: f64 = 1.5;

fn selected(m: &Mask, p: (f64, f64)) -> bool {
    p.0 >= 0.0
        && p.1 >= 0.0
        && p.0 < m.width() as f64
        && p.1 < m.height() as f64
        && m.get(p.0 as u32, p.1 as u32) >= 128
}

fn lerp(a: &StrokePoint, b: &StrokePoint, t: f64) -> StrokePoint {
    let f = t as f32;
    StrokePoint {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        width: a.width + (b.width - a.width) * f,
        opacity: a.opacity + (b.opacity - a.opacity) * f,
    }
}

/// A closed path of straight-edged polygons.
pub fn polygons_path(polys: &[Vec<Pt>]) -> Path {
    Path {
        subpaths: polys
            .iter()
            .filter(|p| p.len() >= 3)
            .map(|p| SubPath {
                anchors: p.iter().map(|q| Anchor::corner(*q)).collect(),
                closed: true,
            })
            .collect(),
    }
}

fn path_polygons(path: &Path) -> Vec<Vec<Pt>> {
    path.flatten(0.25)
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p.len() >= 3)
        .collect()
}

/// Pieces of one stroke, each tagged inside (true) or outside the selection.
fn split_stroke(stroke: &Stroke, m: &Mask) -> Vec<(bool, Stroke)> {
    let mut pts = stroke.points.clone();
    if pts.is_empty() {
        return Vec::new();
    }
    let closed = stroke.closed && pts.len() > 2;
    if closed {
        pts.push(pts[0]);
    }
    let mut runs: Vec<(bool, Vec<StrokePoint>)> = Vec::new();
    let mut state = selected(m, (pts[0].x, pts[0].y));
    let mut current = vec![pts[0]];
    for pair in pts.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let len = (b.x - a.x).hypot(b.y - a.y);
        let steps = (len / SAMPLE_PX).ceil().max(1.0) as usize;
        let mut prev = 0.0;
        for k in 1..=steps {
            let t = k as f64 / steps as f64;
            let p = lerp(a, b, t);
            if selected(m, (p.x, p.y)) != state {
                // Narrow the crossing down between the last two samples.
                let (mut lo, mut hi) = (prev, t);
                for _ in 0..12 {
                    let mid = (lo + hi) / 2.0;
                    let q = lerp(a, b, mid);
                    if selected(m, (q.x, q.y)) == state {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let cut = lerp(a, b, (lo + hi) / 2.0);
                current.push(cut);
                runs.push((state, std::mem::replace(&mut current, vec![cut])));
                state = !state;
            }
            prev = t;
        }
        current.push(*b);
    }
    runs.push((state, current));
    if runs.len() == 1 {
        return vec![(runs[0].0, stroke.clone())];
    }
    // A closed stroke's first and last pieces meet where it started.
    if closed && runs.len() > 2 && runs[0].0 == runs[runs.len() - 1].0 {
        let (_, first) = runs.remove(0);
        let last = runs.last_mut().expect("more than one run");
        last.1.extend(first.into_iter().skip(1));
    }
    runs.into_iter()
        .filter(|(_, p)| p.len() >= 2)
        .map(|(inside, points)| {
            (
                inside,
                Stroke {
                    points,
                    closed: false,
                    ..stroke.clone()
                },
            )
        })
        .collect()
}

/// Split `set` by the document-space `selection`: what lies outside it and
/// what lies inside it, in that order.
pub fn split(set: &StrokeSet, selection: &Mask) -> Result<(StrokeSet, StrokeSet), String> {
    let mut outside = StrokeSet::default();
    let mut inside = StrokeSet::default();
    for stroke in &set.strokes {
        for (is_in, piece) in split_stroke(stroke, selection) {
            if is_in {
                inside.strokes.push(piece);
            } else {
                outside.strokes.push(piece);
            }
        }
    }
    if !set.fills.is_empty() {
        let clip = polygons_path(&trace(selection, 0.5));
        for fill in &set.fills {
            let shape = polygons_path(&fill.outlines);
            for (op, into) in [
                (BooleanOp::Intersect, &mut inside),
                (BooleanOp::Subtract, &mut outside),
            ] {
                let outlines = path_polygons(&boolean(&shape, &clip, op)?);
                if !outlines.is_empty() {
                    into.fills.push(StrokeFill {
                        outlines,
                        color: fill.color,
                    });
                }
            }
        }
    }
    Ok((outside, inside))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select;

    fn line(x0: f64, x1: f64, y: f64) -> Stroke {
        Stroke {
            points: vec![StrokePoint::new(x0, y), StrokePoint::new(x1, y)],
            ..Stroke::new([0, 0, 0, 255], 4.0)
        }
    }

    #[test]
    fn strokes_split_where_they_cross_the_selection() {
        let set = StrokeSet {
            strokes: vec![line(10.0, 90.0, 50.0), line(10.0, 30.0, 10.0)],
            fills: vec![StrokeFill {
                outlines: vec![vec![(0.0, 60.0), (100.0, 60.0), (100.0, 80.0), (0.0, 80.0)]],
                color: [255, 0, 0, 255],
            }],
        };
        let sel = select::rect(100, 100, 40.0, 0.0, 20.0, 100.0);
        let (out, inn) = split(&set, &sel).unwrap();
        assert_eq!(inn.strokes.len(), 1);
        let p = &inn.strokes[0].points;
        assert!((p[0].x - 40.0).abs() < 0.01 && (p.last().unwrap().x - 60.0).abs() < 0.01);
        assert_eq!(out.strokes.len(), 3, "two halves and the untouched line");
        assert!(
            out.strokes
                .iter()
                .all(|s| s.points.iter().all(|p| p.x <= 40.01 || p.x >= 59.99))
        );
        // The fill is clipped into the selection and around it.
        let rendered_in = inn.rasterize(100, 100);
        assert!(rendered_in.get(50, 70)[3] > 60000);
        assert_eq!(rendered_in.get(20, 70)[3], 0);
        let rendered_out = out.rasterize(100, 100);
        assert!(rendered_out.get(20, 70)[3] > 60000);
        assert_eq!(rendered_out.get(50, 70)[3], 0);
        // Width and opacity interpolate at the cut.
        let mut faded = line(0.0, 100.0, 50.0);
        faded.points[1].opacity = 0.0;
        let (_, inn) = split(
            &StrokeSet {
                strokes: vec![faded],
                fills: Vec::new(),
            },
            &sel,
        )
        .unwrap();
        assert!((inn.strokes[0].points[0].opacity - 0.6).abs() < 0.01);
    }

    #[test]
    fn closed_strokes_rejoin_across_their_start() {
        let mut ring = Stroke::new([0, 0, 0, 255], 2.0);
        ring.closed = true;
        ring.points = [(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)]
            .map(|(x, y)| StrokePoint::new(x, y))
            .to_vec();
        // Select the middle band: the ring's left and right sides stay out.
        let sel = select::rect(100, 100, 40.0, 0.0, 20.0, 100.0);
        let (out, inn) = split(
            &StrokeSet {
                strokes: vec![ring.clone()],
                fills: Vec::new(),
            },
            &sel,
        )
        .unwrap();
        assert_eq!(inn.strokes.len(), 2, "top and bottom middles");
        assert_eq!(
            out.strokes.len(),
            2,
            "left and right sides, rejoined at the start"
        );
        // Nothing selected: the whole ring stays, still closed.
        let none = select::rect(100, 100, 0.0, 0.0, 0.0, 0.0);
        let (out, inn) = split(
            &StrokeSet {
                strokes: vec![ring.clone()],
                fills: Vec::new(),
            },
            &none,
        )
        .unwrap();
        assert!(inn.strokes.is_empty());
        assert_eq!(out.strokes, vec![ring]);
    }
}
