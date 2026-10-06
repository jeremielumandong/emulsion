//! Editable design building blocks. Templates contain native paths and text.
#[path = "design_brand.rs"]
pub mod brand;
#[cfg(test)]
#[path = "design_frame_tests.rs"]
mod frame_tests;
#[path = "design_template_families.rs"]
pub mod template_families;
/// Compatibility name for the original invitation-family catalog.
pub use template_families as invitations;
#[path = "design_media.rs"]
pub mod media;
#[path = "design_responsive_templates.rs"]
mod responsive_templates;
#[path = "design_templates.rs"]
mod templates;
#[path = "design_typography.rs"]
mod typography;
use crate::{Command, Document, Node, NodeKind, command::Slot, text::TextSpec};
use emulsion_raster::vector::{Anchor, Path, PathStyle, SubPath};
pub use media::{
    ImageFit, crop_frame_image, fit_frame_image, frame_image_editable, frame_image_replaceable,
};
use std::sync::Arc;
pub use typography::{TypographyPair, TypographyStyle, typography_pair, typography_pairs};

/// A frame is a native group with a vector clipping base and optional image.
/// Group transforms move both together; the image remains independently croppable.
pub fn frame(doc: &Document, element: Element) -> crate::fragment::Fragment {
    let mut shape = element.node((doc.width, doc.height), [226, 230, 235, 255]);
    shape.id = 1;
    shape.parent = Some(2);
    shape.name = "Frame boundary".into();
    let group = Node::new(
        2,
        format!("{} frame", element.label()),
        NodeKind::Group { collapsed: false },
    );
    crate::fragment::Fragment {
        design: Default::default(),
        diagram: None,
        nodes: vec![shape, group],
        roots: vec![2],
        raw_originals: Vec::new(),
    }
}

pub fn frame_parts(
    doc: &Document,
    selected: crate::NodeId,
) -> Option<(crate::NodeId, Option<crate::NodeId>)> {
    let selected = doc.node(selected)?;
    if doc.design.frames.contains_key(&selected.id)
        || selected
            .parent
            .and_then(|id| doc.design.frames.get(&id))
            .is_some_and(|frame| frame.boundary == selected.id)
    {
        return None;
    }
    let members = if selected.is_group() {
        doc.children(Some(selected.id))
    } else {
        doc.children(selected.parent)
    };
    let boundary = match &selected.kind {
        NodeKind::Path { .. } => selected.clip_to.unwrap_or(selected.id),
        NodeKind::Raster { .. } => selected.clip_to?,
        NodeKind::Group { .. } => {
            let mut shapes = members.iter().filter(|id| {
                doc.node(**id)
                    .is_some_and(|n| n.clip_to.is_none() && matches!(n.kind, NodeKind::Path { .. }))
            });
            let boundary = *shapes.next()?;
            // A general illustration group is not an unambiguous media frame.
            if shapes.next().is_some() {
                return None;
            }
            boundary
        }
        _ => return None,
    };
    let base = doc.node(boundary)?;
    if !matches!(base.kind, NodeKind::Path { .. }) {
        return None;
    }
    if matches!(selected.kind, NodeKind::Raster { .. }) {
        return (selected.parent == base.parent).then_some((boundary, Some(selected.id)));
    }
    let image = members
        .iter()
        .find(|id| {
            doc.node(**id).is_some_and(|n| {
                n.clip_to == Some(boundary) && matches!(n.kind, NodeKind::Raster { .. })
            })
        })
        .copied();
    Some((boundary, image))
}

pub fn place_in_frame(
    editor: &mut crate::Editor,
    selected: crate::NodeId,
    raster: Arc<emulsion_raster::Raster>,
) -> Result<crate::NodeId, String> {
    if editor.in_transaction() {
        return Err("Finish the current edit before replacing frame media.".into());
    }
    let (boundary, image) =
        frame_parts(&editor.doc, selected).ok_or("Select a frame or vector shape first.")?;
    media::frame_image_replaceable(&editor.doc, selected)?;
    let previous = image
        .and_then(|id| editor.doc.node(id))
        .and_then(|node| match &node.kind {
            NodeKind::Raster { placement, .. } => Some(*placement),
            _ => None,
        })
        .unwrap_or_default();
    let placement = media::placement(
        &editor.doc,
        boundary,
        &raster,
        ImageFit::Cover,
        [0.5; 2],
        previous,
    )?;
    editor.begin("Replace frame image");
    let result = (|| {
        if let Some(id) = image {
            editor
                .execute(Command::ReplaceContent {
                    id,
                    raster,
                    mask: None,
                    placement,
                    label: "Replace frame image".into(),
                })
                .map_err(|e| e.to_string())?;
            Ok(id)
        } else {
            let parent = editor.doc.node(boundary).unwrap().parent;
            let slot = media::frame_border(&editor.doc, boundary)
                .and_then(|border| {
                    editor
                        .doc
                        .children(parent)
                        .iter()
                        .position(|id| *id == border)
                })
                .map_or(Slot::top_of(parent), |index| Slot { parent, index });
            let id = editor
                .execute(Command::AddNode {
                    node: Box::new(Node::raster(0, "Frame image", raster, placement)),
                    slot,
                })
                .map_err(|e| e.to_string())?
                .ok_or("Image was not created")?;
            editor
                .execute(Command::SetClip {
                    id,
                    clip_to: Some(boundary),
                })
                .map_err(|e| e.to_string())?;
            Ok(id)
        }
    })();
    match result {
        Ok(id) => {
            editor.end();
            Ok(id)
        }
        Err(error) => {
            editor.cancel();
            Err(error)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    Rectangle,
    Circle,
    Triangle,
    Diamond,
    Star,
    Line,
    Arrow,
    Heart,
}
impl Element {
    pub const ALL: [Self; 8] = [
        Self::Rectangle,
        Self::Circle,
        Self::Triangle,
        Self::Diamond,
        Self::Star,
        Self::Line,
        Self::Arrow,
        Self::Heart,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Circle => "Circle",
            Self::Triangle => "Triangle",
            Self::Diamond => "Diamond",
            Self::Star => "Star",
            Self::Line => "Line",
            Self::Arrow => "Arrow",
            Self::Heart => "Heart",
        }
    }
    pub fn path(self, x: f64, y: f64, w: f64, h: f64) -> Path {
        use emulsion_raster::vector_geometry::{ellipse, rectangle};
        let polygon = |points: Vec<(f64, f64)>, closed| Path {
            subpaths: vec![SubPath {
                anchors: points
                    .into_iter()
                    .map(|(a, b)| Anchor::corner((x + a * w, y + b * h)))
                    .collect(),
                closed,
            }],
        };
        match self {
            Self::Rectangle => rectangle(x, y, w, h),
            Self::Circle => ellipse(x, y, w, h),
            Self::Triangle => polygon(vec![(0.5, 0.), (1., 1.), (0., 1.)], true),
            Self::Diamond => polygon(vec![(0.5, 0.), (1., 0.5), (0.5, 1.), (0., 0.5)], true),
            Self::Star => polygon(
                (0..10)
                    .map(|i| {
                        let a = i as f64 * std::f64::consts::PI / 5. - std::f64::consts::FRAC_PI_2;
                        let r = if i % 2 == 0 { 0.5 } else { 0.21 };
                        (0.5 + r * a.cos(), 0.5 + r * a.sin())
                    })
                    .collect(),
                true,
            ),
            Self::Line => polygon(vec![(0., 0.5), (1., 0.5)], false),
            Self::Arrow => polygon(
                vec![
                    (0., 0.32),
                    (0.6, 0.32),
                    (0.6, 0.),
                    (1., 0.5),
                    (0.6, 1.),
                    (0.6, 0.68),
                    (0., 0.68),
                ],
                true,
            ),
            Self::Heart => polygon(
                (0..80)
                    .map(|i| {
                        let t = i as f64 * std::f64::consts::TAU / 80.;
                        (
                            0.5 + 16. * t.sin().powi(3) / 34.,
                            0.48 - (13. * t.cos()
                                - 5. * (2. * t).cos()
                                - 2. * (3. * t).cos()
                                - (4. * t).cos())
                                / 34.,
                        )
                    })
                    .collect(),
                true,
            ),
        }
    }
    pub fn node(self, canvas: (u32, u32), color: [u8; 4]) -> Node {
        let (w, h) = (canvas.0 as f64, canvas.1 as f64);
        let side = w.min(h) * 0.3;
        Node::path(
            0,
            self.label(),
            Arc::new(self.path((w - side) / 2., (h - side) / 2., side, side)),
            PathStyle {
                fill: (self != Self::Line).then_some(color),
                stroke: (self == Self::Line).then_some(color),
                width: (side / 50.).max(1.) as f32,
                ..Default::default()
            },
            canvas.0,
            canvas.1,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TextPreset {
    Heading,
    Subheading,
    Body,
    Caption,
    Quote,
}
impl TextPreset {
    pub const ALL: [Self; 5] = [
        Self::Heading,
        Self::Subheading,
        Self::Body,
        Self::Caption,
        Self::Quote,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Heading => "Add a heading",
            Self::Subheading => "Add a subheading",
            Self::Body => "Add body text",
            Self::Caption => "Add a caption",
            Self::Quote => "Add a quote",
        }
    }
    pub fn node(self, canvas: (u32, u32), font: &str, color: [u8; 4]) -> Node {
        let (ratio, text, bold, italic) = match self {
            Self::Heading => (0.085, "Your heading", true, false),
            Self::Subheading => (0.048, "Your subheading", false, false),
            Self::Body => (
                0.03,
                "Tell your story here. Select this text to make it yours.",
                false,
                false,
            ),
            Self::Caption => (0.022, "A few words that matter", false, false),
            Self::Quote => (0.055, "“Make something meaningful.”", false, true),
        };
        let size = (canvas.0.min(canvas.1) as f32 * ratio).max(8.);
        Node::text(
            0,
            self.label(),
            TextSpec {
                text: text.into(),
                font: font.into(),
                size,
                bold,
                italic,
                color,
                x: canvas.0 as f32 * 0.1,
                y: canvas.1 as f32 * 0.4,
                width: Some(canvas.0 as f32 * 0.8),
                ..Default::default()
            },
            canvas.0,
            canvas.1,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TemplateCategory {
    pub preset: &'static str,
    pub label: &'static str,
    pub tint: u32,
    pub ink: u32,
    pub back: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum Template {
    /// Index in the bundled, data-driven starter catalog.
    Bundled(usize),
    Responsive(usize),
    ProductLaunch,
    SeasonSale,
    Resume,
    DinnerMenu,
    VideoThumbnail,
    PhotoCollage,
    Announcement,
    Editorial,
    Quote,
    Presentation,
    Event,
    BusinessCard,
}
impl Template {
    pub const CATEGORIES: [TemplateCategory; 12] = [
        TemplateCategory {
            preset: "Square post",
            label: "Instagram Post",
            tint: 0xf9d3dc,
            ink: 0x55182a,
            back: 0xf1a2b5,
        },
        TemplateCategory {
            preset: "Portrait post",
            label: "Portrait Post",
            tint: 0xe7d6f8,
            ink: 0x3a1f5e,
            back: 0xc9a8ef,
        },
        TemplateCategory {
            preset: "Story",
            label: "Your Story",
            tint: 0xf9d6e4,
            ink: 0x5a1d3a,
            back: 0xefa6c4,
        },
        TemplateCategory {
            preset: "Classic",
            label: "Certificate & Quote",
            tint: 0xd5dcfa,
            ink: 0x1c285a,
            back: 0xa7b5f2,
        },
        TemplateCategory {
            preset: "Widescreen",
            label: "Presentation",
            tint: 0xfbd9c5,
            ink: 0x4a2412,
            back: 0xf0b48c,
        },
        TemplateCategory {
            preset: "Business card",
            label: "Business Card",
            tint: 0xd7ddf9,
            ink: 0x1e2a5c,
            back: 0xa9b6f0,
        },
        TemplateCategory {
            preset: "A4 flyer",
            label: "Resume & Flyer",
            tint: 0xe4d9f9,
            ink: 0x33205c,
            back: 0xc1acf0,
        },
        TemplateCategory {
            preset: "Poster",
            label: "Poster",
            tint: 0xe9d7f6,
            ink: 0x3c1d58,
            back: 0xcba9ec,
        },
        TemplateCategory {
            preset: "Video thumbnail",
            label: "Video Thumbnail",
            tint: 0xf8d5ee,
            ink: 0x521b46,
            back: 0xeaa4d6,
        },
        TemplateCategory {
            preset: "Banner",
            label: "Banner",
            tint: 0xfdf0c4,
            ink: 0x5a4310,
            back: 0xf2d67c,
        },
        TemplateCategory {
            preset: "Invitation",
            label: "Invitation",
            tint: 0xe8d8f6,
            ink: 0x3a1f5e,
            back: 0xcba9ec,
        },
        TemplateCategory {
            preset: "Responsive",
            label: "Responsive layouts",
            tint: 0xd6e7fa,
            ink: 0x203759,
            back: 0xa6c7ef,
        },
    ];
    pub fn catalog() -> impl Iterator<Item = Self> {
        (0..templates::starters().len())
            .map(Self::Bundled)
            .chain((0..responsive_templates::NAMES.len()).map(Self::Responsive))
    }
    pub fn category(self) -> Option<usize> {
        if matches!(self, Self::Responsive(_)) {
            return Some(11);
        }
        let spec = templates::spec(self)?;
        Self::CATEGORIES
            .iter()
            .position(|category| category.preset == spec.preset)
    }
    pub const ALL: [Self; 10] = [
        Self::ProductLaunch,
        Self::SeasonSale,
        Self::Event,
        Self::Quote,
        Self::Presentation,
        Self::BusinessCard,
        Self::Resume,
        Self::DinnerMenu,
        Self::VideoThumbnail,
        Self::PhotoCollage,
    ];
    pub const ADDITIONAL: [Self; 2] = [Self::Announcement, Self::Editorial];
    pub fn label(self) -> &'static str {
        match self {
            Self::Responsive(index) => responsive_templates::NAMES
                .get(index)
                .copied()
                .unwrap_or("Responsive template"),
            Self::Bundled(_) => {
                templates::spec(self).map_or("Template", |spec| spec.label.as_str())
            }
            Self::ProductLaunch => "Product launch",
            Self::SeasonSale => "Season sale",
            Self::Resume => "Resume",
            Self::DinnerMenu => "Dinner menu",
            Self::VideoThumbnail => "Video thumbnail",
            Self::PhotoCollage => "Photo collage",
            Self::Announcement => "Bold announcement",
            Self::Editorial => "Editorial story",
            Self::Quote => "Daily inspiration",
            Self::Presentation => "Presentation cover",
            Self::Event => "Event invitation",
            Self::BusinessCard => "Business card",
        }
    }
    pub fn native_size(self) -> (u32, u32) {
        if matches!(self, Self::Responsive(_)) {
            return (1200, 1200);
        }
        templates::spec(self).map_or((1080, 1080), |spec| (spec.w, spec.h))
    }
    pub fn create(self, w: u32, h: u32) -> Result<Document, String> {
        if let Self::Responsive(index) = self {
            return responsive_templates::create(index, w, h);
        }
        if let Some(spec) = templates::spec(self) {
            return spec.create(w, h);
        }
        if matches!(self, Self::Bundled(_)) {
            return Err("Unknown bundled template".into());
        }
        let mut doc = crate::creation::CanvasSpec {
            width: w as f64,
            height: h as f64,
            ..Default::default()
        }
        .create()?;
        let dark = matches!(self, Self::Announcement | Self::Presentation | Self::Event);
        let bg = if dark {
            [24, 26, 35, 255]
        } else {
            [246, 242, 232, 255]
        };
        doc.nodes[0].kind = NodeKind::Fill { rgba: bg };
        let ink = if dark {
            [250, 249, 246, 255]
        } else {
            [29, 32, 40, 255]
        };
        let accent = [230, 103, 69, 255];
        let wf = w as f64;
        let hf = h as f64;
        let shape = if matches!(self, Self::Quote | Self::Event) {
            Element::Circle
        } else {
            Element::Rectangle
        };
        let path = shape.path(wf * 0.65, -hf * 0.12, wf * 0.52, hf * 0.8);
        Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Accent",
                Arc::new(path),
                PathStyle {
                    fill: Some(accent),
                    stroke: None,
                    ..Default::default()
                },
                w,
                h,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .map_err(|e| e.to_string())?;
        let (eyebrow, title, body) = match self {
            Self::Announcement => (
                "SOMETHING NEW",
                "Big ideas.\nMade real.",
                "Your announcement starts here.",
            ),
            Self::Editorial => (
                "THE JOURNAL",
                "A fresh\nperspective",
                "Stories, ideas, and things worth sharing.",
            ),
            Self::Quote => (
                "WORDS TO LIVE BY",
                "Make space\nfor possibility.",
                "Your name · Your story",
            ),
            Self::Presentation => (
                "YOUR NEXT CHAPTER",
                "Ideas that\nmove us.",
                "Presentation title · Presenter · Date",
            ),
            Self::Event => (
                "YOU ARE INVITED",
                "Let's make\na moment.",
                "Event name · Date · Place",
            ),
            Self::BusinessCard => (
                "YOUR BUSINESS",
                "Your name",
                "Your role\nhello@example.com · Your website",
            ),
            _ => unreachable!("bundled starter handled above"),
        };
        for (name, text, y, size, bold) in [
            ("Eyebrow", eyebrow, 0.12, 0.024, true),
            ("Title", title, 0.28, 0.088, true),
            ("Description", body, 0.73, 0.027, false),
        ] {
            let spec = TextSpec {
                text: text.into(),
                font: "Geist".into(),
                size: (w.min(h) as f32 * size).max(6.),
                bold,
                color: ink,
                x: w as f32 * 0.09,
                y: h as f32 * y,
                width: Some(w as f32 * 0.8),
                ..Default::default()
            };
            Command::AddNode {
                node: Box::new(Node::text(0, name, spec, w, h)),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .map_err(|e| e.to_string())?;
        }
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_replaces_source_without_flattening_and_undo_restores_crop() {
        let mut editor = crate::Editor::new(Document::new(600, 400), None);
        let group = frame(&editor.doc, Element::Circle)
            .paste(&mut editor, Slot::TOP, (0., 0.))
            .unwrap()[0];
        let raster = Arc::new(emulsion_raster::Raster::solid(120, 80, [0.2, 0.3, 0.4, 1.]));
        let image = place_in_frame(&mut editor, group, raster.clone()).unwrap();
        assert!(
            matches!(editor.doc.node(image).unwrap().kind,NodeKind::Raster{raster:ref r,..} if Arc::ptr_eq(r,&raster))
        );
        let before = editor.doc.clone();
        let replacement = Arc::new(emulsion_raster::Raster::solid(30, 90, [0.7, 0.2, 0.1, 1.]));
        assert_eq!(
            place_in_frame(&mut editor, group, replacement).unwrap(),
            image
        );
        editor.undo();
        assert_eq!(editor.doc, before);
        editor
            .execute(Command::TranslateNode {
                id: group,
                dx: 20.,
                dy: 10.,
            })
            .unwrap();
        editor.doc.validate().unwrap();
        assert!(editor.doc.node(image).unwrap().clip_to.is_some());
    }
    #[test]
    fn templates_and_elements_are_native_valid_and_scalable() {
        for template in Template::catalog() {
            let doc = template.create(1080, 1080).unwrap();
            doc.validate().unwrap();
            assert!(
                doc.nodes
                    .iter()
                    .filter(|n| matches!(n.kind, NodeKind::Text { .. }))
                    .count()
                    >= 2
            );
            assert!(
                doc.nodes
                    .iter()
                    .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
            );
        }
        for element in Element::ALL {
            let mut doc = Document::new(600, 400);
            Command::AddNode {
                node: Box::new(element.node((600, 400), [20, 40, 60, 255])),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
            doc.validate().unwrap();
            assert!(
                crate::geometry::node_bounds(&doc, doc.nodes[0].id)
                    .unwrap()
                    .is_some()
            );
        }
    }
}
