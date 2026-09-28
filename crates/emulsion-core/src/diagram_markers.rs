//! Editable connector markers and smooth connector geometry.
use super::*;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    None,
    #[default]
    Block,
    Classic,
    Open,
    Diamond,
    Oval,
    CirclePlus,
    Many,
    One,
    MandatoryOne,
    ZeroToOne,
    ZeroToMany,
    OneToMany,
}
impl MarkerKind {
    pub const ALL: [Self; 13] = [
        Self::None,
        Self::Block,
        Self::Classic,
        Self::Open,
        Self::Diamond,
        Self::Oval,
        Self::CirclePlus,
        Self::Many,
        Self::One,
        Self::MandatoryOne,
        Self::ZeroToOne,
        Self::ZeroToMany,
        Self::OneToMany,
    ];
    pub fn drawio(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Block => "block",
            Self::Classic => "classic",
            Self::Open => "open",
            Self::Diamond => "diamond",
            Self::Oval => "oval",
            Self::CirclePlus => "circlePlus",
            Self::Many => "ERmany",
            Self::One => "ERone",
            Self::MandatoryOne => "ERmandOne",
            Self::ZeroToOne => "ERzeroToOne",
            Self::ZeroToMany => "ERzeroToMany",
            Self::OneToMany => "ERoneToMany",
        }
    }
    pub fn from_drawio(s: &str) -> Option<Self> {
        Some(match s {
            "none" => Self::None,
            "block" | "blockThin" => Self::Block,
            "classic" | "classicThin" => Self::Classic,
            "open" | "openThin" => Self::Open,
            "diamond" | "diamondThin" => Self::Diamond,
            "oval" | "circle" => Self::Oval,
            "circlePlus" => Self::CirclePlus,
            "ERmany" => Self::Many,
            "ERone" => Self::One,
            "ERmandOne" => Self::MandatoryOne,
            "ERzeroToOne" => Self::ZeroToOne,
            "ERzeroToMany" => Self::ZeroToMany,
            "ERoneToMany" => Self::OneToMany,
            _ => return None,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Marker {
    pub kind: MarkerKind,
    pub filled: bool,
    pub size: f64,
}
impl Default for Marker {
    fn default() -> Self {
        Self {
            kind: MarkerKind::Block,
            filled: true,
            size: 10.,
        }
    }
}
impl Marker {
    pub fn valid(self) -> bool {
        self.size.is_finite() && (1.0..=100.).contains(&self.size)
    }
    pub(super) fn inset(self) -> f64 {
        if self.filled
            && matches!(
                self.kind,
                MarkerKind::Block | MarkerKind::Classic | MarkerKind::Diamond | MarkerKind::Oval
            )
        {
            return 0.;
        }
        match self.kind {
            MarkerKind::Block
            | MarkerKind::Classic
            | MarkerKind::Diamond
            | MarkerKind::Oval
            | MarkerKind::CirclePlus => self.size,
            MarkerKind::ZeroToOne | MarkerKind::ZeroToMany => self.size * 1.95,
            _ => 0.,
        }
    }
    pub fn path(self, tip: (f64, f64), prior: (f64, f64), width: f64) -> Path {
        let mut out = Path::default();
        let s = self.size;
        let thick = width.max(0.75).min(s / 2.);
        let polygon = |p: &[(f64, f64)]| SubPath {
            anchors: p.iter().copied().map(Anchor::corner).collect(),
            closed: true,
        };
        let line = |out: &mut Path, a: (f64, f64), b: (f64, f64)| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = dx.hypot(dy).max(1e-6);
            let (nx, ny) = (-dy / l * thick / 2., dx / l * thick / 2.);
            out.subpaths.push(polygon(&[
                (a.0 + nx, a.1 + ny),
                (b.0 + nx, b.1 + ny),
                (b.0 - nx, b.1 - ny),
                (a.0 - nx, a.1 - ny),
            ]));
        };
        let ring = |out: &mut Path, cx: f64, r: f64, filled: bool| {
            out.subpaths.extend(
                crate::design::Element::Circle
                    .path(cx - r, -r, r * 2., r * 2.)
                    .subpaths,
            );
            if !filled && r > thick {
                let mut inside = crate::design::Element::Circle.path(
                    cx - r + thick,
                    -r + thick,
                    2. * (r - thick),
                    2. * (r - thick),
                );
                for sub in &mut inside.subpaths {
                    sub.anchors.reverse();
                    for a in &mut sub.anchors {
                        std::mem::swap(&mut a.h_in, &mut a.h_out);
                    }
                }
                out.subpaths.extend(inside.subpaths);
            }
        };
        match self.kind {
            MarkerKind::None => {}
            MarkerKind::Block | MarkerKind::Classic | MarkerKind::Diamond => {
                let pts = match self.kind {
                    MarkerKind::Diamond => {
                        vec![(0., 0.), (-s / 2., s / 3.), (-s, 0.), (-s / 2., -s / 3.)]
                    }
                    MarkerKind::Classic => {
                        vec![(0., 0.), (-s, s / 2.), (-s * 0.75, 0.), (-s, -s / 2.)]
                    }
                    _ => vec![(0., 0.), (-s, s / 2.), (-s, -s / 2.)],
                };
                if self.filled {
                    out.subpaths.push(polygon(&pts));
                } else {
                    for i in 0..pts.len() {
                        line(&mut out, pts[i], pts[(i + 1) % pts.len()]);
                    }
                }
            }
            MarkerKind::Open => {
                line(&mut out, (-s, s / 2.), (0., 0.));
                line(&mut out, (0., 0.), (-s, -s / 2.));
            }
            MarkerKind::Oval | MarkerKind::CirclePlus => {
                ring(
                    &mut out,
                    -s / 2.,
                    s / 2.,
                    self.kind == MarkerKind::Oval && self.filled,
                );
                if self.kind == MarkerKind::CirclePlus {
                    line(&mut out, (-s, 0.), (0., 0.));
                    line(&mut out, (-s / 2., -s / 2.), (-s / 2., s / 2.));
                }
            }
            kind => {
                let many = matches!(
                    kind,
                    MarkerKind::Many | MarkerKind::ZeroToMany | MarkerKind::OneToMany
                );
                if many {
                    for y in [-s / 2., 0., s / 2.] {
                        line(&mut out, (0., y), (-s, 0.));
                    }
                } else {
                    line(&mut out, (-s / 3., -s / 2.), (-s / 3., s / 2.));
                }
                if matches!(kind, MarkerKind::MandatoryOne | MarkerKind::OneToMany) {
                    line(&mut out, (-s * 1.25, -s / 2.), (-s * 1.25, s / 2.));
                }
                if matches!(kind, MarkerKind::ZeroToOne | MarkerKind::ZeroToMany) {
                    ring(&mut out, -s * 1.6, s * 0.35, false);
                }
            }
        }
        let angle = (tip.1 - prior.1).atan2(tip.0 - prior.0);
        out.transform(
            glam::DAffine2::from_translation(glam::dvec2(tip.0, tip.1))
                * glam::DAffine2::from_angle(angle),
        );
        out
    }
}
pub(super) fn curved(points: &[(f64, f64)]) -> Path {
    let mut anchors: Vec<_> = points.iter().copied().map(Anchor::corner).collect();
    if points.len() == 2 {
        let a = points[0];
        let b = points[1];
        let mid = (a.0 + b.0) / 2.;
        anchors[0].h_out = (mid, a.1);
        anchors[1].h_in = (mid, b.1);
    } else {
        for i in 0..points.len() {
            let prev = points[i.saturating_sub(1)];
            let next = points[(i + 1).min(points.len() - 1)];
            let delta = ((next.0 - prev.0) / 6., (next.1 - prev.1) / 6.);
            anchors[i].h_in = (points[i].0 - delta.0, points[i].1 - delta.1);
            anchors[i].h_out = (points[i].0 + delta.0, points[i].1 + delta.1);
        }
    }
    Path {
        subpaths: vec![SubPath {
            anchors,
            closed: false,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_marker_handles_minimum_size_and_hollow_geometry() {
        for kind in MarkerKind::ALL {
            for size in [1., 10., 100.] {
                for filled in [true, false] {
                    let marker = Marker { kind, size, filled };
                    assert!(marker.valid());
                    let path = marker.path((30., 40.), (20., 25.), 4.);
                    assert!(
                        path.subpaths
                            .iter()
                            .flat_map(|s| &s.anchors)
                            .all(|a| a.p.0.is_finite() && a.p.1.is_finite())
                    );
                }
            }
        }
    }
}

/// Round polyline corners without changing the attachment endpoints.
pub(super) fn rounded(points: &[(f64, f64)], radius: f64) -> Path {
    let mut anchors = vec![Anchor::corner(points[0])];
    for triple in points.windows(3) {
        let [a, b, c] = [triple[0], triple[1], triple[2]];
        let incoming = (a.0-b.0, a.1-b.1);
        let outgoing = (c.0-b.0, c.1-b.1);
        let l1 = incoming.0.hypot(incoming.1);
        let l2 = outgoing.0.hypot(outgoing.1);
        if l1 < 1e-6 || l2 < 1e-6 { continue; }
        let r = radius.min(l1/2.).min(l2/2.);
        let mut entry = Anchor::corner((b.0+incoming.0*r/l1, b.1+incoming.1*r/l1));
        let mut exit = Anchor::corner((b.0+outgoing.0*r/l2, b.1+outgoing.1*r/l2));
        entry.h_out = (entry.p.0+(b.0-entry.p.0)*2./3., entry.p.1+(b.1-entry.p.1)*2./3.);
        exit.h_in = (exit.p.0+(b.0-exit.p.0)*2./3., exit.p.1+(b.1-exit.p.1)*2./3.);
        anchors.extend([entry, exit]);
    }
    anchors.push(Anchor::corner(*points.last().unwrap()));
    Path { subpaths: vec![SubPath { anchors, closed: false }] }
}
