//! Art-directed native layouts on a 500 × 700 design grid.
use super::{FamilyId, Palette, Selection, VariantId, family};
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
pub(super) enum Card {
    Invitation,
    Details,
    Rsvp,
}
#[derive(Clone, Copy)]
enum Face {
    Serif,
    Italic,
    Sans,
    Display,
}
struct Art {
    doc: Document,
    nodes: Vec<Node>,
    sx: f64,
    sy: f64,
    unit: f64,
}
impl Art {
    fn new(width: u32, height: u32, background: Color) -> Result<Self, String> {
        let mut doc = CanvasSpec {
            width: width as f64,
            height: height as f64,
            resolution: 300.,
            kind: CanvasKind::Design,
            depth: 8,
            ..Default::default()
        }
        .create()?;
        doc.nodes[0].name = "Paper".into();
        doc.nodes[0].kind = NodeKind::Fill { rgba: background };
        let sx = width as f64 / 500.;
        let sy = height as f64 / 700.;
        Ok(Self {
            doc,
            nodes: Vec::new(),
            sx,
            sy,
            unit: sx.min(sy),
        })
    }
    fn finish(mut self) -> Result<Document, String> {
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
    fn path(
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
    fn rect(&mut self, name: &str, r: [f64; 4], color: Color) {
        self.path(
            name,
            crate::design::Element::Rectangle.path(r[0], r[1], r[2], r[3]),
            Some(color),
            None,
            0.,
        );
    }
    fn outline(&mut self, name: &str, r: [f64; 4], color: Color, weight: f64) {
        self.path(
            name,
            crate::design::Element::Rectangle.path(r[0], r[1], r[2], r[3]),
            None,
            Some(color),
            weight,
        );
    }
    fn ellipse(&mut self, name: &str, r: [f64; 4], color: Color) {
        self.path(
            name,
            crate::design::Element::Circle.path(r[0], r[1], r[2], r[3]),
            Some(color),
            None,
            0.,
        );
    }
    fn line(&mut self, name: &str, a: (f64, f64), b: (f64, f64), color: Color, width: f64) {
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
    fn polygon(&mut self, name: &str, points: &[(f64, f64)], color: Color) {
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
    fn arch(&mut self, name: &str, r: [f64; 4], fill: Option<Color>, stroke: Option<Color>) {
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
    fn star(&mut self, name: &str, x: f64, y: f64, radius: f64, points: usize, color: Color) {
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
    fn leaf(&mut self, name: &str, from: (f64, f64), tip: (f64, f64), width: f64, color: Color) {
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
    fn sprig(&mut self, x: f64, y: f64, scale: f64, mirror: f64, color: Color) {
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
    fn text(
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
    fn type_with_tracking(
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
    fn caps(&mut self, name: &str, text: &str, r: [f64; 4], color: Color, align: Align) {
        self.type_with_tracking(name, text, r, Face::Sans, color, align, 1.65);
    }
}

pub(super) fn create(
    selection: Selection,
    card: Card,
    width: u32,
    height: u32,
) -> Result<Document, String> {
    let p = *selection.palette()?;
    let mut a = Art::new(width, height, p.background)?;
    match card {
        Card::Invitation => match selection.family {
            FamilyId::GardenVows => garden(&mut a, selection.variant, p),
            FamilyId::ModernVows => modern(&mut a, selection.variant, p),
            FamilyId::ConfettiClub => confetti(&mut a, selection.variant, p),
            FamilyId::MidnightToast => midnight(&mut a, selection.variant, p),
        },
        Card::Details | Card::Rsvp => companion(&mut a, selection, card, p),
    }
    a.finish()
}

fn garden(a: &mut Art, v: VariantId, p: Palette) {
    use Align::{Center, Left};
    match v {
        VariantId::BotanicalArch => {
            a.arch("Ivory arch", [35., 38., 430., 626.], Some(p.surface), None);
            a.arch(
                "Fine arch border",
                [48., 51., 404., 600.],
                None,
                Some(p.secondary),
            );
            a.sprig(76., 244., 0.72, 1., p.accent);
            a.sprig(424., 589., 0.72, -1., p.accent);
            a.caps(
                "Invitation heading",
                "TOGETHER WITH OUR FAMILIES",
                [90., 166., 320., 9.],
                p.ink,
                Center,
            );
            a.text(
                "First name",
                "Alexandra",
                [74., 215., 352., 54.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Ampersand",
                "&",
                [200., 282., 100., 31.],
                Face::Italic,
                p.accent,
                Center,
            );
            a.text(
                "Second name",
                "Oliver",
                [80., 324., 340., 58.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Invitation line",
                "invite you to celebrate their wedding",
                [91., 407., 318., 13.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.line("Date ornament", (221., 453.), (279., 453.), p.accent, 1.);
            a.caps(
                "Wedding date",
                "SATURDAY · 17 JULY 2027",
                [90., 478., 320., 11.],
                p.ink,
                Center,
            );
            a.text(
                "Wedding time",
                "at four o’clock in the afternoon",
                [92., 507., 316., 14.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Wedding venue",
                "THE GLASSHOUSE",
                [90., 552., 320., 12.],
                Face::Sans,
                p.ink,
                Center,
            );
            a.text(
                "Wedding location",
                "Brooklyn, New York",
                [90., 577., 320., 12.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Reception note",
                "Dinner & dancing to follow",
                [90., 613., 320., 13.],
                Face::Italic,
                p.accent,
                Center,
            );
        }
        VariantId::GardenBorder => {
            a.rect("Cotton paper", [0., 0., 500., 700.], p.surface);
            a.outline(
                "Outer botanical frame",
                [30., 30., 440., 640.],
                p.accent,
                0.9,
            );
            a.outline(
                "Inner botanical frame",
                [38., 38., 424., 624.],
                p.secondary,
                0.7,
            );
            a.sprig(64., 193., 0.94, 1., p.accent);
            a.sprig(436., 638., 0.94, -1., p.accent);
            a.text(
                "Couple monogram",
                "A / O",
                [146., 86., 208., 30.],
                Face::Serif,
                p.accent,
                Center,
            );
            a.caps(
                "Invitation heading",
                "JOYFULLY INVITE YOU",
                [90., 161., 320., 10.],
                p.ink,
                Center,
            );
            a.text(
                "Couple names",
                "Alexandra & Oliver",
                [70., 225., 360., 44.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Invitation line",
                "to the celebration of their marriage",
                [80., 296., 340., 14.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.line("Date rule left", (86., 422.), (151., 422.), p.secondary, 1.);
            a.line(
                "Date rule right",
                (349., 422.),
                (414., 422.),
                p.secondary,
                1.,
            );
            a.text(
                "Wedding day",
                "17",
                [155., 349., 190., 112.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.caps(
                "Wedding month",
                "JULY 2027",
                [150., 496., 200., 10.],
                p.ink,
                Center,
            );
            a.text(
                "Wedding venue",
                "The Glasshouse · Brooklyn",
                [85., 560., 330., 16.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Wedding time",
                "Four in the afternoon",
                [85., 589., 330., 13.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.caps(
                "Reception note",
                "RECEPTION TO FOLLOW",
                [90., 625., 320., 8.5],
                p.accent,
                Center,
            );
        }
        VariantId::WildflowerEditorial => {
            a.rect("Editorial paper", [0., 0., 370., 700.], p.surface);
            a.rect("Botanical column", [370., 0., 130., 700.], p.accent);
            a.sprig(422., 335., 1.1, 1., p.surface);
            a.sprig(454., 526., 0.86, -1., p.secondary);
            a.caps(
                "Invitation heading",
                "THE WEDDING OF",
                [42., 66., 278., 10.],
                p.ink,
                Left,
            );
            a.line("Heading rule", (42., 97.), (320., 97.), p.secondary, 1.);
            a.text(
                "First name",
                "Alexandra",
                [40., 173., 306., 49.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Ampersand",
                "&",
                [44., 238., 240., 40.],
                Face::Italic,
                p.accent,
                Left,
            );
            a.text(
                "Second name",
                "Oliver",
                [40., 295., 298., 65.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Invitation line",
                "A day for love.\nAn evening to remember.",
                [44., 407., 282., 18.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.caps(
                "Wedding date",
                "17 JULY 2027",
                [44., 500., 280., 11.],
                p.ink,
                Left,
            );
            a.text(
                "Wedding time",
                "Saturday at four in the afternoon",
                [44., 534., 282., 12.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Wedding venue",
                "The Glasshouse\nBrooklyn, New York",
                [44., 577., 282., 16.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Reception note",
                "Dinner & dancing to follow",
                [44., 642., 282., 12.],
                Face::Italic,
                p.accent,
                Left,
            );
        }
        _ => unreachable!("validated Garden Vows variant"),
    }
}

fn modern(a: &mut Art, v: VariantId, p: Palette) {
    use Align::{Center, Left, Right};
    match v {
        VariantId::SplitType => {
            a.caps(
                "Invitation heading",
                "A CELEBRATION OF LOVE",
                [38., 46., 424., 9.],
                p.ink,
                Left,
            );
            a.line("Heading rule", (38., 78.), (462., 78.), p.ink, 1.);
            a.text(
                "First name",
                "Alexandra",
                [36., 135., 430., 66.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Ampersand",
                "&",
                [40., 222., 130., 61.],
                Face::Italic,
                p.accent,
                Left,
            );
            a.text(
                "Second name",
                "Oliver",
                [36., 306., 428., 88.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.rect("Lower color block", [0., 444., 500., 256.], p.ink);
            a.caps(
                "Wedding date",
                "17 JULY\n2027",
                [39., 479., 187., 15.],
                p.surface,
                Left,
            );
            a.line(
                "Information divider",
                (245., 481.),
                (245., 642.),
                p.secondary,
                1.,
            );
            a.text(
                "Wedding venue",
                "The Glasshouse",
                [277., 480., 188., 22.],
                Face::Serif,
                p.surface,
                Left,
            );
            a.text(
                "Wedding location",
                "Brooklyn, New York",
                [278., 522., 184., 11.],
                Face::Sans,
                p.surface,
                Left,
            );
            a.text(
                "Wedding time",
                "Saturday\nFour in the afternoon",
                [278., 568., 184., 13.],
                Face::Serif,
                p.surface,
                Left,
            );
            a.caps(
                "Reception note",
                "DINNER & DANCING",
                [38., 656., 426., 8.5],
                p.surface,
                Left,
            );
        }
        VariantId::Monogram => {
            a.outline("Architectural border", [26., 26., 448., 648.], p.ink, 1.);
            a.ellipse("Monogram seal", [183., 73., 134., 134.], p.ink);
            a.text(
                "Couple monogram",
                "A / O",
                [188., 112., 124., 33.],
                Face::Serif,
                p.surface,
                Center,
            );
            a.caps(
                "Invitation heading",
                "WE ARE GETTING MARRIED",
                [70., 248., 360., 9.],
                p.ink,
                Center,
            );
            a.text(
                "First name",
                "Alexandra",
                [50., 306., 400., 57.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Second name",
                "& Oliver",
                [50., 374., 400., 57.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.line("Short rule", (216., 468.), (284., 468.), p.accent, 1.);
            a.caps(
                "Wedding date",
                "17 · 07 · 27",
                [85., 501., 330., 14.],
                p.ink,
                Center,
            );
            a.text(
                "Wedding venue",
                "The Glasshouse, Brooklyn",
                [72., 548., 356., 18.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Wedding time",
                "Four in the afternoon",
                [80., 581., 340., 11.],
                Face::Sans,
                p.ink,
                Center,
            );
            a.caps(
                "Reception note",
                "A NIGHT TO REMEMBER",
                [65., 638., 370., 8.5],
                p.ink,
                Center,
            );
        }
        VariantId::Gallery => {
            a.rect("Date column", [0., 0., 147., 700.], p.ink);
            a.caps(
                "Date column heading",
                "JULY",
                [20., 49., 107., 11.],
                p.surface,
                Center,
            );
            a.text(
                "Wedding day",
                "17",
                [12., 108., 123., 90.],
                Face::Serif,
                p.surface,
                Center,
            );
            a.line(
                "Date column rule",
                (34., 246.),
                (113., 246.),
                p.secondary,
                1.,
            );
            a.text(
                "Wedding year",
                "2027",
                [20., 275., 107., 28.],
                Face::Serif,
                p.surface,
                Center,
            );
            a.caps(
                "Date column footer",
                "A / O",
                [20., 630., 107., 11.],
                p.surface,
                Center,
            );
            a.caps(
                "Invitation heading",
                "YOU ARE INVITED",
                [178., 52., 291., 9.],
                p.ink,
                Left,
            );
            a.text(
                "First name",
                "Alexandra",
                [174., 179., 304., 46.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Ampersand",
                "&",
                [178., 245., 280., 44.],
                Face::Italic,
                p.accent,
                Left,
            );
            a.text(
                "Second name",
                "Oliver",
                [174., 310., 294., 67.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Invitation line",
                "Together with our families,\njoin us for our wedding.",
                [179., 423., 282., 14.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.line("Venue rule", (179., 493.), (462., 493.), p.secondary, 1.);
            a.text(
                "Wedding venue",
                "The Glasshouse",
                [179., 525., 282., 27.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Wedding location",
                "Brooklyn, New York",
                [179., 568., 282., 12.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Wedding time",
                "4 PM · DINNER TO FOLLOW",
                [179., 641., 282., 9.],
                Face::Sans,
                p.ink,
                Right,
            );
        }
        _ => unreachable!("validated Modern Heirloom variant"),
    }
}

fn confetti_bits(a: &mut Art, p: Palette) {
    for (x, y, r, color) in [
        (48., 101., 7., p.accent),
        (438., 149., 10., p.secondary),
        (53., 473., 9., p.secondary),
        (448., 441., 6., p.accent),
        (394., 71., 5., p.ink),
    ] {
        a.ellipse("Round confetti", [x - r, y - r, r * 2., r * 2.], color);
    }
    a.star("Confetti star", 77., 207., 17., 4, p.accent);
    a.star("Confetti star", 427., 332., 20., 4, p.ink);
    a.polygon(
        "Paper confetti",
        &[(60., 344.), (70., 336.), (90., 358.), (80., 366.)],
        p.secondary,
    );
    a.polygon(
        "Paper confetti",
        &[(397., 526.), (405., 513.), (429., 528.), (421., 541.)],
        p.accent,
    );
}
fn confetti(a: &mut Art, v: VariantId, p: Palette) {
    use Align::{Center, Left};
    match v {
        VariantId::BigNumber => {
            confetti_bits(a, p);
            a.caps(
                "Invitation heading",
                "YOU’RE INVITED TO",
                [80., 54., 340., 10.],
                p.ink,
                Center,
            );
            a.text(
                "Birthday name",
                "Avery’s",
                [75., 105., 350., 56.],
                Face::Display,
                p.ink,
                Center,
            );
            a.ellipse("Birthday age badge", [102., 198., 296., 296.], p.accent);
            a.text(
                "Birthday age",
                "30",
                [112., 251., 276., 176.],
                Face::Display,
                p.surface,
                Center,
            );
            a.text(
                "Birthday headline",
                "birthday party",
                [58., 514., 384., 33.],
                Face::Display,
                p.ink,
                Center,
            );
            a.caps(
                "Birthday date",
                "SATURDAY · 24 JULY · 7 PM",
                [55., 578., 390., 10.],
                p.ink,
                Center,
            );
            a.text(
                "Birthday venue",
                "The Rooftop · 42 Garden Street",
                [55., 614., 390., 14.],
                Face::Sans,
                p.ink,
                Center,
            );
            a.text(
                "Birthday note",
                "Good people. Great music. Cake, obviously.",
                [53., 651., 394., 11.],
                Face::Sans,
                p.ink,
                Center,
            );
        }
        VariantId::PartyTicket => {
            a.rect("Ticket paper", [34., 35., 432., 630.], p.surface);
            a.outline("Ticket edge", [34., 35., 432., 630.], p.ink, 1.7);
            a.rect("Admission strip", [34., 35., 432., 68.], p.accent);
            a.caps(
                "Admission heading",
                "ADMIT ONE · A VERY GOOD TIME",
                [53., 61., 394., 9.],
                p.surface,
                Center,
            );
            a.text(
                "Birthday headline",
                "LET’S\nPARTY",
                [59., 134., 382., 65.],
                Face::Display,
                p.ink,
                Left,
            );
            a.star("Birthday starburst", 371., 361., 61., 12, p.secondary);
            a.text(
                "Birthday age",
                "30",
                [329., 331., 85., 41.],
                Face::Display,
                p.ink,
                Center,
            );
            a.text(
                "Birthday name",
                "Avery is\nturning thirty.",
                [63., 328., 235., 25.],
                Face::Display,
                p.ink,
                Left,
            );
            a.text(
                "Birthday note",
                "And you’re on the guest list.",
                [63., 405., 366., 13.],
                Face::Sans,
                p.ink,
                Left,
            );
            for i in 0..20 {
                let x = 43. + i as f64 * 21.;
                a.line("Ticket perforation", (x, 461.), (x + 10., 461.), p.ink, 1.);
            }
            a.ellipse("Left ticket notch", [19., 447., 28., 28.], p.background);
            a.ellipse("Right ticket notch", [453., 447., 28., 28.], p.background);
            a.caps("Date label", "WHEN", [64., 493., 110., 8.], p.accent, Left);
            a.text(
                "Birthday date",
                "JUL 24",
                [63., 516., 180., 30.],
                Face::Display,
                p.ink,
                Left,
            );
            a.text(
                "Birthday time",
                "SATURDAY · 7 PM",
                [64., 563., 180., 10.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.line(
                "Ticket stub divider",
                (254., 494.),
                (254., 589.),
                p.secondary,
                1.,
            );
            a.caps(
                "Venue label",
                "WHERE",
                [280., 493., 151., 8.],
                p.accent,
                Left,
            );
            a.text(
                "Birthday venue",
                "The Rooftop",
                [279., 523., 151., 18.],
                Face::Display,
                p.ink,
                Left,
            );
            a.text(
                "Birthday location",
                "42 Garden Street",
                [280., 563., 151., 11.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.caps(
                "Ticket footer",
                "DANCING SHOES ENCOURAGED",
                [64., 621., 366., 8.],
                p.ink,
                Center,
            );
        }
        VariantId::ShapeStack => {
            a.rect("Top accent block", [0., 0., 500., 118.], p.accent);
            a.caps(
                "Invitation heading",
                "AVERY’S 30TH BIRTHDAY",
                [39., 45., 422., 12.],
                p.surface,
                Left,
            );
            a.rect("Display side block", [390., 118., 110., 320.], p.secondary);
            a.star("Graphic party star", 435., 191., 41., 6, p.ink);
            a.text(
                "Birthday headline",
                "OH,\nWHAT A\nPARTY!",
                [35., 147., 351., 60.],
                Face::Display,
                p.ink,
                Left,
            );
            a.ellipse("Bottom circle", [356., 383., 92., 92.], p.accent);
            a.rect("Info card", [31., 493., 438., 176.], p.surface);
            a.text(
                "Birthday date",
                "24 JULY",
                [52., 520., 397., 35.],
                Face::Display,
                p.ink,
                Left,
            );
            a.caps(
                "Birthday time",
                "SATURDAY · 7 PM UNTIL LATE",
                [54., 575., 389., 9.],
                p.ink,
                Left,
            );
            a.text(
                "Birthday venue",
                "The Rooftop · 42 Garden Street",
                [54., 614., 389., 13.],
                Face::Sans,
                p.ink,
                Left,
            );
        }
        _ => unreachable!("validated Confetti Club variant"),
    }
}

fn deco_frame(a: &mut Art, p: Palette) {
    a.outline("Deco outer frame", [23., 23., 454., 654.], p.accent, 1.);
    a.outline("Deco inner frame", [32., 32., 436., 636.], p.secondary, 0.8);
    for (x, sign) in [(46., 1.), (454., -1.)] {
        a.line("Deco corner", (x, 82.), (x, 46.), p.accent, 1.2);
        a.line(
            "Deco corner",
            (x, 46.),
            (x + sign * 40., 46.),
            p.accent,
            1.2,
        );
        a.line("Deco corner", (x, 618.), (x, 654.), p.accent, 1.2);
        a.line(
            "Deco corner",
            (x, 654.),
            (x + sign * 40., 654.),
            p.accent,
            1.2,
        );
    }
}
fn midnight(a: &mut Art, v: VariantId, p: Palette) {
    use Align::{Center, Left};
    match v {
        VariantId::Moonlight => {
            a.ellipse("Moon roundel", [145., 61., 210., 210.], p.accent);
            a.ellipse("Moon inset", [157., 73., 186., 186.], p.background);
            a.text(
                "Birthday age",
                "30",
                [160., 101., 180., 112.],
                Face::Serif,
                p.accent,
                Center,
            );
            a.star("Evening sparkle", 403., 135., 13., 4, p.accent);
            a.star("Evening sparkle", 95., 243., 8., 4, p.accent);
            a.caps(
                "Invitation heading",
                "AN EVENING IN GOOD COMPANY",
                [68., 312., 364., 9.],
                p.ink,
                Center,
            );
            a.text(
                "Birthday headline",
                "A toast\nto Avery",
                [64., 352., 372., 59.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.line("Date rule", (218., 503.), (282., 503.), p.accent, 1.);
            a.caps(
                "Birthday date",
                "24 JULY 2027 · 7 PM",
                [68., 538., 364., 10.],
                p.accent,
                Center,
            );
            a.text(
                "Birthday venue",
                "The Rooftop",
                [68., 580., 364., 27.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Birthday location",
                "42 Garden Street · Brooklyn",
                [68., 620., 364., 11.],
                Face::Sans,
                p.ink,
                Center,
            );
            a.text(
                "Birthday note",
                "Dinner, drinks & a little dancing",
                [68., 658., 364., 12.],
                Face::Italic,
                p.accent,
                Center,
            );
        }
        VariantId::ArtDeco => {
            deco_frame(a, p);
            a.caps(
                "Invitation heading",
                "PLEASE JOIN US TO CELEBRATE",
                [62., 81., 376., 8.5],
                p.accent,
                Center,
            );
            a.text(
                "Birthday name",
                "AVERY",
                [72., 139., 356., 48.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.polygon(
                "Geometric age halo",
                &[(250., 209.), (399., 357.), (250., 505.), (101., 357.)],
                p.surface,
            );
            a.text(
                "Birthday age",
                "30",
                [119., 241., 262., 168.],
                Face::Serif,
                p.accent,
                Center,
            );
            a.caps(
                "Birthday age caption",
                "YEARS OF WONDERFUL",
                [94., 442., 312., 8.5],
                p.ink,
                Center,
            );
            a.caps(
                "Birthday date",
                "SATURDAY · 24 JULY 2027",
                [69., 531., 362., 9.],
                p.accent,
                Center,
            );
            a.text(
                "Birthday venue",
                "The Rooftop, Brooklyn",
                [63., 572., 374., 24.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Birthday time",
                "Seven in the evening",
                [75., 613., 350., 14.],
                Face::Italic,
                p.accent,
                Center,
            );
        }
        VariantId::SupperClub => {
            a.rect("Supper club side panel", [345., 0., 155., 700.], p.accent);
            a.text(
                "Birthday age",
                "30",
                [356., 71., 135., 90.],
                Face::Serif,
                p.background,
                Center,
            );
            a.line(
                "Side panel rule",
                (381., 212.),
                (464., 212.),
                p.background,
                0.9,
            );
            a.caps(
                "Side panel month",
                "JULY",
                [365., 252., 115., 11.],
                p.background,
                Center,
            );
            a.text(
                "Side panel day",
                "24",
                [355., 291., 135., 62.],
                Face::Serif,
                p.background,
                Center,
            );
            a.star("Supper club star", 423., 522., 39., 8, p.background);
            a.caps(
                "Invitation heading",
                "THE SUPPER CLUB",
                [36., 51., 275., 10.],
                p.accent,
                Left,
            );
            a.text(
                "Birthday headline",
                "Here’s\nto the\ngood life.",
                [33., 150., 296., 58.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Birthday name",
                "Celebrating Avery’s 30th",
                [39., 404., 273., 20.],
                Face::Italic,
                p.accent,
                Left,
            );
            a.line("Venue rule", (39., 467.), (307., 467.), p.secondary, 1.);
            a.text(
                "Birthday venue",
                "The Rooftop",
                [38., 507., 277., 31.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Birthday location",
                "42 Garden Street\nBrooklyn, New York",
                [39., 553., 274., 12.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.caps(
                "Birthday time",
                "SATURDAY · 7 PM",
                [39., 635., 274., 10.],
                p.accent,
                Left,
            );
        }
        _ => unreachable!("validated Midnight Toast variant"),
    }
}

fn companion(a: &mut Art, selection: Selection, card: Card, p: Palette) {
    use Align::{Center, Left};
    let wedding = family(selection.family).occasion == super::Occasion::Wedding;
    let rsvp = matches!(card, Card::Rsvp);
    let title = if rsvp { "Kindly reply" } else { "The details" };
    let names = if wedding {
        "ALEXANDRA & OLIVER"
    } else {
        "AVERY’S 30TH BIRTHDAY"
    };
    let (face, content_ink, label_ink) = match selection.family {
        FamilyId::GardenVows => {
            a.rect("Companion cotton paper", [0., 0., 500., 700.], p.surface);
            a.outline(
                "Botanical companion border",
                [28., 28., 444., 644.],
                p.secondary,
                0.85,
            );
            a.sprig(57., 177., 0.63, 1., p.accent);
            a.sprig(443., 637., 0.65, -1., p.accent);
            a.caps("Set name", names, [93., 74., 314., 9.], p.ink, Center);
            a.text(
                "Card title",
                title,
                [71., 135., 358., 51.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.line("Card title rule", (222., 208.), (278., 208.), p.accent, 1.);
            (Face::Serif, p.ink, p.accent)
        }
        FamilyId::ModernVows => {
            a.rect("Companion header", [0., 0., 500., 110.], p.ink);
            a.caps("Set name", names, [52., 46., 396., 10.], p.surface, Left);
            a.text(
                "Card title",
                title,
                [49., 145., 402., 56.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.line("Card title rule", (53., 221.), (447., 221.), p.ink, 1.);
            (Face::Serif, p.ink, p.ink)
        }
        FamilyId::ConfettiClub => {
            a.rect("Companion top block", [0., 0., 500., 112.], p.accent);
            a.caps("Set name", names, [43., 45., 414., 10.], p.surface, Left);
            a.rect("Companion paper", [31., 139., 438., 533.], p.surface);
            a.text(
                "Card title",
                title,
                [51., 163., 396., 39.],
                Face::Display,
                p.ink,
                Left,
            );
            a.star("Companion sparkle", 439., 112., 22., 6, p.secondary);
            a.ellipse("Companion confetti", [9., 553., 14., 14.], p.accent);
            (Face::Sans, p.ink, p.ink)
        }
        FamilyId::MidnightToast => {
            deco_frame(a, p);
            a.caps("Set name", names, [65., 79., 370., 9.], p.accent, Center);
            a.text(
                "Card title",
                title,
                [61., 137., 378., 53.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.star("Companion title ornament", 250., 214., 9., 4, p.accent);
            (Face::Serif, p.ink, p.accent)
        }
    };
    let margin = if matches!(
        selection.family,
        FamilyId::GardenVows | FamilyId::MidnightToast
    ) {
        74.
    } else {
        54.
    };
    let width = 500. - margin * 2.;
    if rsvp {
        let date = if wedding {
            "Please respond by 5 July 2027"
        } else {
            "Please respond by 10 July 2027"
        };
        a.text(
            "RSVP deadline",
            date,
            [margin, 248., width, 16.],
            face,
            content_ink,
            Left,
        );
        a.caps(
            "Guest names label",
            "YOUR NAME(S)",
            [margin, 305., width, 9.],
            label_ink,
            Left,
        );
        a.line(
            "Guest names writing line",
            (margin, 348.),
            (500. - margin, 348.),
            p.secondary,
            1.,
        );
        a.outline(
            "Accept response box",
            [margin, 388., 12., 12.],
            label_ink,
            1.,
        );
        a.text(
            "Accept response",
            "Joyfully accepts",
            [margin + 25., 384., width - 25., 16.],
            face,
            content_ink,
            Left,
        );
        a.outline(
            "Decline response box",
            [margin, 429., 12., 12.],
            label_ink,
            1.,
        );
        a.text(
            "Decline response",
            "Regretfully declines",
            [margin + 25., 425., width - 25., 16.],
            face,
            content_ink,
            Left,
        );
        a.caps(
            "Dietary requirements label",
            "DIETARY REQUIREMENTS",
            [margin, 486., width, 9.],
            label_ink,
            Left,
        );
        a.line(
            "Dietary requirements writing line",
            (margin, 530.),
            (500. - margin, 530.),
            p.secondary,
            1.,
        );
        a.text(
            "RSVP return instructions",
            "Please return this card to the hosts\nor reply through our event website.",
            [margin, 574., width, 14.],
            face,
            content_ink,
            Left,
        );
        a.text(
            "RSVP closing",
            "We can’t wait to celebrate with you.",
            [margin, 630., width, 13.],
            Face::Italic,
            label_ink,
            Left,
        );
    } else {
        a.text(
            "Event date",
            if wedding {
                "Saturday, 17 July 2027"
            } else {
                "Saturday, 24 July 2027"
            },
            [margin, 248., width, 18.],
            face,
            content_ink,
            Left,
        );
        let sections = if wedding {
            [
                (
                    "THE CEREMONY",
                    "Arrive at 3:30 PM for a 4 PM ceremony.\nThe Glasshouse · Brooklyn, New York.",
                ),
                (
                    "THE CELEBRATION",
                    "Cocktails, dinner and dancing to follow.\nGarden party attire; bring your dancing shoes.",
                ),
                (
                    "GETTING HERE",
                    "The venue is a short walk from the station.\nPlease see our wedding website for travel details.",
                ),
            ]
        } else {
            [
                (
                    "THE PLAN",
                    "Drinks at 7 PM, dinner at 8 PM.\nStay for music, cake and a little dancing.",
                ),
                (
                    "THE PLACE",
                    "The Rooftop · 42 Garden Street.\nTake the lift to the top floor.",
                ),
                (
                    "A LITTLE NOTE",
                    "Come dressed for a celebration.\nYour company is the best birthday gift.",
                ),
            ]
        };
        for (i, (label, body)) in sections.into_iter().enumerate() {
            let y = 307. + i as f64 * 106.;
            a.caps(
                "Details section heading",
                label,
                [margin, y, width, 8.5],
                label_ink,
                Left,
            );
            a.text(
                "Details section text",
                body,
                [margin, y + 29., width, 13.],
                face,
                content_ink,
                Left,
            );
        }
        a.line(
            "Companion footer rule",
            (margin, 615.),
            (500. - margin, 615.),
            p.secondary,
            1.,
        );
        a.text(
            "Companion closing",
            "Good company. A beautiful occasion.",
            [margin, 638., width, 13.],
            Face::Italic,
            label_ink,
            Left,
        );
    }
}
