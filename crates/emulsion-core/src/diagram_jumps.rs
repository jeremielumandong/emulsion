//! Editable line jumps. Spatial queries limit crossing tests to nearby segments.
use super::*;
use rstar::{AABB, RTree, RTreeObject};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JumpStyle {
    #[default]
    None,
    Arc,
    Gap,
    Sharp,
}
impl JumpStyle {
    pub fn drawio(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Arc => "arc",
            Self::Gap => "gap",
            Self::Sharp => "sharp",
        }
    }
}
#[derive(Clone)]
struct Segment {
    edge: u64,
    a: (f64, f64),
    b: (f64, f64),
}
impl RTreeObject for Segment {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(
            [self.a.0.min(self.b.0), self.a.1.min(self.b.1)],
            [self.a.0.max(self.b.0), self.a.1.max(self.b.1)],
        )
    }
}
fn cross(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.1 - a.1 * b.0
}
fn subtract(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 - b.0, a.1 - b.1)
}
fn intersection(a: &Segment, b: &Segment) -> Option<f64> {
    let r = subtract(a.b, a.a);
    let s = subtract(b.b, b.a);
    let d = cross(r, s);
    if d.abs() < 1e-9 {
        return None;
    }
    let delta = subtract(b.a, a.a);
    let t = cross(delta, s) / d;
    let u = cross(delta, r) / d;
    ((1e-6..1. - 1e-6).contains(&t) && (1e-6..1. - 1e-6).contains(&u)).then_some(t * r.0.hypot(r.1))
}
pub(super) fn apply(doc: &mut Document, model: &Diagram) {
    if !model
        .edges
        .values()
        .any(|e| e.jump_style != JumpStyle::None)
    {
        return;
    }
    let indices = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect::<HashMap<_, _>>();
    let paths = model
        .edges
        .iter()
        .filter_map(|(id, e)| match &doc.nodes[indices[&e.path]].kind {
            NodeKind::Path { path, .. } => Some((*id, path.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let segments = paths
        .iter()
        .flat_map(|(id, p)| {
            p.flatten(0.5).into_iter().flat_map(move |(points, _)| {
                points
                    .windows(2)
                    .map(|pair| Segment {
                        edge: *id,
                        a: pair[0],
                        b: pair[1],
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let tree = RTree::bulk_load(segments);
    for (id, edge) in &model.edges {
        if edge.jump_style == JumpStyle::None || matches!(edge.routing, Routing::Curved | Routing::Cyclical) {
            continue;
        }
        let Some(path) = paths.get(id) else {
            continue;
        };
        let mut result = Path::default();
        for sub in &path.subpaths {
            if sub.closed || sub.anchors.is_empty() {
                result.subpaths.push(sub.clone());
                continue;
            }
            let mut line = SubPath {
                anchors: vec![sub.anchors[0]],
                closed: false,
            };
            for pair in sub.anchors.windows(2) {
                if pair[0].h_out != pair[0].p || pair[1].h_in != pair[1].p {
                    line.anchors.push(pair[1]);
                    continue;
                }
                let seg = Segment {
                    edge: *id,
                    a: pair[0].p,
                    b: pair[1].p,
                };
                let delta = subtract(seg.b, seg.a);
                let length = delta.0.hypot(delta.1);
                if length < 1e-6 {
                    continue;
                }
                let radius = (edge.jump_size / 2.).min(length / 4.);
                let mut hits = tree
                    .locate_in_envelope_intersecting(&seg.envelope())
                    .filter(|s| {
                        s.edge != *id
                            && (s.edge < *id || model.edges[&s.edge].jump_style == JumpStyle::None)
                    })
                    .filter_map(|s| intersection(&seg, s))
                    .filter(|d| *d > radius && *d < length - radius)
                    .collect::<Vec<_>>();
                hits.sort_by(f64::total_cmp);
                hits.dedup_by(|a, b| (*a - *b).abs() < radius * 2.);
                let unit = (delta.0 / length, delta.1 / length);
                let at = |d: f64, normal: f64| {
                    (
                        seg.a.0 + unit.0 * d - unit.1 * normal,
                        seg.a.1 + unit.1 * d + unit.0 * normal,
                    )
                };
                for d in hits {
                    line.anchors.push(Anchor::corner(at(d - radius, 0.)));
                    match edge.jump_style {
                        JumpStyle::Gap => {
                            result.subpaths.push(line);
                            line = SubPath {
                                anchors: vec![Anchor::corner(at(d + radius, 0.))],
                                closed: false,
                            };
                        }
                        JumpStyle::Sharp => {
                            line.anchors.push(Anchor::corner(at(d, -radius)));
                            line.anchors.push(Anchor::corner(at(d + radius, 0.)));
                        }
                        JumpStyle::Arc => {
                            let last = line.anchors.last_mut().unwrap();
                            last.h_out = at(d - radius, -radius * 1.3333333333);
                            let mut end = Anchor::corner(at(d + radius, 0.));
                            end.h_in = at(d + radius, -radius * 1.3333333333);
                            line.anchors.push(end);
                        }
                        JumpStyle::None => {}
                    }
                }
                line.anchors.push(pair[1]);
            }
            result.subpaths.push(line);
        }
        let (w, h) = (doc.width, doc.height);
        if let NodeKind::Path { path, style, cache } = &mut doc.nodes[indices[&edge.path]].kind
            && **path != result
        {
            *path = Arc::new(result);
            *cache = crate::vector_cache::VectorRaster::path(path.clone(), *style, w, h);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crossing_excludes_endpoints_and_parallel_lines() {
        let a = Segment {
            edge: 1,
            a: (0., 0.),
            b: (100., 0.),
        };
        assert_eq!(
            intersection(
                &a,
                &Segment {
                    edge: 2,
                    a: (50., -50.),
                    b: (50., 50.)
                }
            ),
            Some(50.)
        );
        assert_eq!(
            intersection(
                &a,
                &Segment {
                    edge: 2,
                    a: (100., 0.),
                    b: (100., 50.)
                }
            ),
            None
        );
        assert_eq!(
            intersection(
                &a,
                &Segment {
                    edge: 2,
                    a: (0., 10.),
                    b: (100., 10.)
                }
            ),
            None
        );
    }
}
