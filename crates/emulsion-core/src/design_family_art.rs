//! Shared original, editable native artwork primitives for curated template families.
use crate::{
    Command, Document, Node, NodeKind,
    command::Slot,
    creation::{CanvasKind, CanvasSpec},
    text::{Align, TextSpec},
};
use emulsion_raster::vector::{Anchor, Path, PathStyle, SubPath};
use std::sync::Arc;

type Color = [u8; 4];
const SERIF: &str = "Cormorant Garamond";
const DISPLAY: &str = "Fraunces";
#[derive(Clone, Copy)]
pub(super) enum Face {
    Serif,
    Italic,
    Sans,
    Display,
}
pub(super) struct Art {
    doc: Document,
    nodes: Vec<Node>,
    sx: f64,
    sy: f64,
    unit: f64,
}
impl Art {
    pub(super) fn new(width: u32, height: u32, background: Color) -> Result<Self, String> {
        Self::with_resolution(width, height, (500., 700.), 300., background)
    }
    /// Screen-first layouts use a category-specific grid at 72 pixels per inch.
    pub(super) fn with_grid(
        width: u32,
        height: u32,
        grid: (f64, f64),
        background: Color,
    ) -> Result<Self, String> {
        Self::with_resolution(width, height, grid, 72., background)
    }
    fn with_resolution(
        width: u32,
        height: u32,
        grid: (f64, f64),
        resolution: f64,
        background: Color,
    ) -> Result<Self, String> {
        let mut doc = CanvasSpec {
            width: width as f64,
            height: height as f64,
            resolution,
            kind: CanvasKind::Design,
            depth: 8,
            ..Default::default()
        }
        .create()?;
        doc.nodes[0].name = "Paper".into();
        doc.nodes[0].kind = NodeKind::Fill { rgba: background };
        let sx = width as f64 / grid.0;
        let sy = height as f64 / grid.1;
        Ok(Self {
            doc,
            nodes: Vec::new(),
            sx,
            sy,
            unit: sx.min(sy),
        })
    }
    pub(super) fn finish(mut self) -> Result<Document, String> {
        for node in self.nodes {
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut self.doc)
            .map_err(|e| e.to_string())?;
        }
        self.doc.validate().map_err(|e| e.to_string())?;
        Ok(self.doc)
    }
    pub(super) fn path(
        &mut self,
        name: &str,
        mut path: Path,
        fill: Option<Color>,
        stroke: Option<Color>,
        width: f64,
    ) {
        path.transform(glam::DAffine2::from_scale(glam::dvec2(self.sx, self.sy)));
        self.nodes.push(Node::path(
            0,
            name,
            Arc::new(path),
            PathStyle {
                fill,
                stroke,
                width: (width * self.unit) as f32,
                ..Default::default()
            },
            self.doc.width,
            self.doc.height,
        ));
    }
    pub(super) fn rect(&mut self, name: &str, r: [f64; 4], color: Color) {
        self.path(
            name,
            crate::design::Element::Rectangle.path(r[0], r[1], r[2], r[3]),
            Some(color),
            None,
            0.,
        );
    }
    pub(super) fn outline(&mut self, name: &str, r: [f64; 4], color: Color, weight: f64) {
        self.path(
            name,
            crate::design::Element::Rectangle.path(r[0], r[1], r[2], r[3]),
            None,
            Some(color),
            weight,
        );
    }
    pub(super) fn ellipse(&mut self, name: &str, r: [f64; 4], color: Color) {
        self.path(
            name,
            crate::design::Element::Circle.path(r[0], r[1], r[2], r[3]),
            Some(color),
            None,
            0.,
        );
    }
    pub(super) fn line(
        &mut self,
        name: &str,
        a: (f64, f64),
        b: (f64, f64),
        color: Color,
        width: f64,
    ) {
        self.path(
            name,
            Path {
                subpaths: vec![SubPath {
                    anchors: vec![Anchor::corner(a), Anchor::corner(b)],
                    closed: false,
                }],
            },
            None,
            Some(color),
            width,
        );
    }
    pub(super) fn polygon(&mut self, name: &str, points: &[(f64, f64)], color: Color) {
        self.path(
            name,
            Path {
                subpaths: vec![SubPath {
                    anchors: points.iter().copied().map(Anchor::corner).collect(),
                    closed: true,
                }],
            },
            Some(color),
            None,
            0.,
        );
    }
    pub(super) fn arch(
        &mut self,
        name: &str,
        r: [f64; 4],
        fill: Option<Color>,
        stroke: Option<Color>,
    ) {
        let [x, y, w, h] = r;
        let rx = w / 2.;
        let k = 0.552_284_749_8;
        let mut anchors = vec![
            Anchor::corner((x, y + h)),
            Anchor::corner((x, y + rx)),
            Anchor::corner((x + rx, y)),
            Anchor::corner((x + w, y + rx)),
            Anchor::corner((x + w, y + h)),
        ];
        anchors[1].h_out = (x, y + rx * (1. - k));
        anchors[2].h_in = (x + rx * (1. - k), y);
        anchors[2].h_out = (x + rx * (1. + k), y);
        anchors[3].h_in = (x + w, y + rx * (1. - k));
        self.path(
            name,
            Path {
                subpaths: vec![SubPath {
                    anchors,
                    closed: true,
                }],
            },
            fill,
            stroke,
            1.,
        );
    }
    pub(super) fn star(
        &mut self,
        name: &str,
        x: f64,
        y: f64,
        radius: f64,
        points: usize,
        color: Color,
    ) {
        let vertices: Vec<_> = (0..points * 2)
            .map(|i| {
                let a =
                    i as f64 * std::f64::consts::PI / points as f64 - std::f64::consts::FRAC_PI_2;
                let r = if i % 2 == 0 { radius } else { radius * 0.53 };
                (x + a.cos() * r, y + a.sin() * r)
            })
            .collect();
        self.polygon(name, &vertices, color);
    }
    /// Filled two-cubic leaf; all handles remain editable in the vector editor.
    pub(super) fn leaf(
        &mut self,
        name: &str,
        from: (f64, f64),
        tip: (f64, f64),
        width: f64,
        color: Color,
    ) {
        let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
        let length = dx.hypot(dy);
        if length == 0. {
            return;
        }
        let normal = (-dy / length * width, dx / length * width);
        let a = Anchor {
            p: from,
            h_in: (from.0 + dx * 0.55 - normal.0, from.1 + dy * 0.55 - normal.1),
            h_out: (from.0 + dx * 0.3 + normal.0, from.1 + dy * 0.3 + normal.1),
            smooth: false,
        };
        let b = Anchor {
            p: tip,
            h_in: (from.0 + dx * 0.7 + normal.0, from.1 + dy * 0.7 + normal.1),
            h_out: (from.0 + dx * 0.45 - normal.0, from.1 + dy * 0.45 - normal.1),
            smooth: false,
        };
        self.path(
            name,
            Path {
                subpaths: vec![SubPath {
                    anchors: vec![a, b],
                    closed: true,
                }],
            },
            Some(color),
            None,
            0.,
        );
    }
    pub(super) fn sprig(&mut self, x: f64, y: f64, scale: f64, mirror: f64, color: Color) {
        let point = |a: f64, b: f64| (x + a * scale * mirror, y + b * scale);
        self.line(
            "Botanical stem",
            point(0., 0.),
            point(18., -120.),
            color,
            0.85,
        );
        for (a, b, c, d) in [
            (3., -20., -19., -45.),
            (6., -39., 34., -61.),
            (9., -62., -12., -88.),
            (12., -81., 37., -104.),
            (15., -102., 12., -133.),
        ] {
            self.leaf(
                "Botanical leaf",
                point(a, b),
                point(c, d),
                6. * scale,
                color,
            );
        }
    }
    pub(super) fn text(
        &mut self,
        name: &str,
        content: &str,
        r: [f64; 4],
        face: Face,
        color: Color,
        align: Align,
    ) {
        self.type_with_tracking(name, content, r, face, color, align, 0.);
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn type_with_tracking(
        &mut self,
        name: &str,
        content: &str,
        r: [f64; 4],
        face: Face,
        color: Color,
        align: Align,
        tracking: f64,
    ) {
        let spec = TextSpec {
            text: content.into(),
            font: match face {
                Face::Serif | Face::Italic => SERIF,
                Face::Display => DISPLAY,
                _ => "Geist",
            }
            .into(),
            size: (r[3] * self.unit) as f32,
            line_height: 1.06,
            color,
            bold: matches!(face, Face::Display),
            italic: matches!(face, Face::Italic),
            align,
            x: (r[0] * self.sx) as f32,
            y: (r[1] * self.sy) as f32,
            width: Some((r[2] * self.sx) as f32),
            letter_spacing: (tracking * self.unit) as f32,
            ..Default::default()
        };
        self.nodes
            .push(Node::text(0, name, spec, self.doc.width, self.doc.height));
    }
    pub(super) fn caps(&mut self, name: &str, text: &str, r: [f64; 4], color: Color, align: Align) {
        self.type_with_tracking(name, text, r, Face::Sans, color, align, 1.65);
    }
}
