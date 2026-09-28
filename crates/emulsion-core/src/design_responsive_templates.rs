//! Additional native starters that demonstrate editable adaptive page layouts.
use super::*;
use crate::{
    Editor,
    design_layout::{self as layout, Breakpoint, Child, Flow, Frame, FrameOverrides},
    design_metadata::{Anchor as ResizeAnchor, Constraint},
};
pub(super) const NAMES: [&str; 10] = [
    "Product overview",
    "Studio portfolio",
    "Event schedule",
    "Course outline",
    "Team introduction",
    "Service packages",
    "Project roadmap",
    "Travel guide",
    "Community update",
    "Research brief",
];
const COPY: [(&str, &str, [&str; 3], [u8; 4]); 10] = [
    (
        "Meet what’s next.",
        "A clear introduction to your next release.",
        ["Discover", "Create", "Share"],
        [66, 82, 211, 255],
    ),
    (
        "Work with purpose.",
        "Selected ideas, thoughtful craft, lasting results.",
        ["Identity", "Digital", "Editorial"],
        [176, 66, 96, 255],
    ),
    (
        "Make a day of it.",
        "Your program, sessions and useful details in one place.",
        ["Welcome", "Sessions", "Connect"],
        [153, 71, 29, 255],
    ),
    (
        "Learn by making.",
        "A practical path from your first idea to a finished project.",
        ["Explore", "Practice", "Present"],
        [34, 112, 85, 255],
    ),
    (
        "Better together.",
        "The people and skills behind our shared work.",
        ["Strategy", "Design", "Delivery"],
        [60, 83, 147, 255],
    ),
    (
        "Choose your pace.",
        "Simple packages that meet you where you are.",
        ["Essential", "Professional", "Complete"],
        [113, 64, 156, 255],
    ),
    (
        "From here to next.",
        "A shared direction with clear milestones and room to grow.",
        ["Now", "Next", "Later"],
        [15, 113, 136, 255],
    ),
    (
        "Take the scenic route.",
        "A local guide to places, flavors and moments worth keeping.",
        ["See", "Taste", "Stay"],
        [160, 92, 35, 255],
    ),
    (
        "Good things happening.",
        "News, upcoming events and ways to take part.",
        ["This month", "Coming up", "Get involved"],
        [157, 61, 89, 255],
    ),
    (
        "Make insight useful.",
        "Turn evidence into a clear story and a practical next step.",
        ["Question", "Evidence", "Next steps"],
        [43, 109, 123, 255],
    ),
];
fn add(e: &mut Editor, parent: Option<u64>, node: Node) -> Result<u64, String> {
    e.execute(Command::AddNode {
        node: Box::new(node),
        slot: Slot::top_of(parent),
    })
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Missing template object".into())
}
#[allow(clippy::too_many_arguments)] // A single native text object’s typography and frame.
fn text(
    e: &mut Editor,
    parent: u64,
    name: &str,
    content: &str,
    size: f32,
    color: [u8; 4],
    bold: bool,
    width: f32,
) -> Result<u64, String> {
    add(
        e,
        Some(parent),
        Node::text(
            0,
            name,
            TextSpec {
                text: content.into(),
                size,
                color,
                bold,
                width: Some(width),
                line_height: 1.18,
                ..Default::default()
            },
            e.doc.width,
            e.doc.height,
        ),
    )
}
fn order(e: &mut Editor, parent: u64, ids: &[u64]) -> Result<(), String> {
    // Native children and layout flow are both ordered from bottom to top.
    for id in ids {
        e.execute(Command::MoveNode {
            id: *id,
            slot: Slot::top_of(Some(parent)),
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn transparent_frame(e: &mut Editor, group: u64) -> Result<(), String> {
    let id = e.doc.design.frames[&group].boundary;
    let NodeKind::Path { path, .. } = &e.doc.node(id).unwrap().kind else {
        unreachable!()
    };
    e.execute(Command::SetPath {
        id,
        path: path.clone(),
        style: PathStyle {
            fill: None,
            stroke: None,
            ..Default::default()
        },
    })
    .map_err(|err| err.to_string())?;
    Ok(())
}
pub(super) fn create(index: usize, w: u32, h: u32) -> Result<Document, String> {
    let (headline, description, labels, accent) =
        COPY.get(index).ok_or("Unknown responsive template")?;
    let mut doc = crate::creation::CanvasSpec {
        width: w as f64,
        height: h as f64,
        ..Default::default()
    }
    .create()?;
    doc.nodes[0].kind = NodeKind::Fill {
        rgba: [247, 246, 242, 255],
    };
    let mut e = Editor::new(doc, None);
    e.begin("Responsive starter");
    let root = add(&mut e, None, Node::group(0, NAMES[index]))?;
    let width = (f64::from(w) - 64.).max(160.);
    let eyebrow = text(
        &mut e,
        root,
        "Eyebrow",
        &format!("EMULSION / {}", NAMES[index].to_uppercase()),
        14.,
        *accent,
        true,
        width as f32,
    )?;
    let title = text(
        &mut e,
        root,
        "Headline",
        headline,
        56.,
        [29, 32, 40, 255],
        true,
        width as f32,
    )?;
    let description = text(
        &mut e,
        root,
        "Introduction",
        description,
        22.,
        [85, 89, 99, 255],
        false,
        width as f32,
    )?;
    let cards = add(&mut e, Some(root), Node::group(0, "Adaptive cards"))?;
    let mut card_ids = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        let card = add(&mut e, Some(cards), Node::group(0, *label))?;
        let number = text(
            &mut e,
            card,
            "Number",
            &format!("0{}", i + 1),
            52.,
            *accent,
            true,
            240.,
        )?;
        let label = text(
            &mut e,
            card,
            "Card title",
            label,
            25.,
            [29, 32, 40, 255],
            true,
            240.,
        )?;
        let body = text(
            &mut e,
            card,
            "Card description",
            "Add a concise detail here. Keep the useful information easy to find.",
            18.,
            [85, 89, 99, 255],
            false,
            240.,
        )?;
        order(&mut e, card, &[number, label, body])?;
        let frame = Frame {
            hug_height: true,
            padding: [24.; 4],
            gap: 12.,
            children: [number, label, body]
                .into_iter()
                .map(|id| {
                    (
                        id,
                        Child {
                            fill_width: true,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            ..Default::default()
        };
        layout::enable(&mut e, card, frame, (320., 240.))?;
        let boundary = e.doc.design.frames[&card].boundary;
        let NodeKind::Path { path, .. } = &e.doc.node(boundary).unwrap().kind else {
            unreachable!()
        };
        e.execute(Command::SetPath {
            id: boundary,
            path: path.clone(),
            style: PathStyle {
                fill: Some([255, 255, 255, 255]),
                stroke: None,
                ..Default::default()
            },
        })
        .map_err(|e| e.to_string())?;
        card_ids.push(card);
    }
    order(&mut e, cards, &card_ids)?;
    let row = Frame {
        flow: Flow::Column,
        hug_height: true,
        padding: [0.; 4],
        gap: 16.,
        children: card_ids
            .iter()
            .map(|id| {
                (
                    *id,
                    Child {
                        fill_width: true,
                        ..Default::default()
                    },
                )
            })
            .collect(),
        breakpoints: vec![Breakpoint {
            min_width: 760.,
            overrides: FrameOverrides {
                flow: Some(Flow::Grid),
                columns: Some(3),
                gap: Some(24.),
                ..Default::default()
            },
        }],
        ..Default::default()
    };
    layout::enable(&mut e, cards, row, (width, 320.))?;
    transparent_frame(&mut e, cards)?;
    let footer = text(
        &mut e,
        root,
        "Footer",
        "YOUR NAME  ·  YOUR WEBSITE  ·  YOUR NEXT STEP",
        13.,
        *accent,
        true,
        width as f32,
    )?;
    order(&mut e, root, &[eyebrow, title, description, cards, footer])?;
    let frame = Frame {
        hug_height: true,
        padding: [32.; 4],
        gap: 24.,
        children: [eyebrow, title, description, cards, footer]
            .into_iter()
            .map(|id| {
                (
                    id,
                    Child {
                        fill_width: true,
                        ..Default::default()
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    layout::enable(
        &mut e,
        root,
        frame,
        (f64::from(w).max(224.), f64::from(h).max(100.)),
    )?;
    transparent_frame(&mut e, root)?;
    let mut design = e.doc.design.clone();
    design.constraints.insert(
        root,
        Constraint {
            horizontal: ResizeAnchor::Stretch,
            vertical: ResizeAnchor::Start,
            reflow_text: true,
        },
    );
    e.execute(Command::SetDesign {
        design: Box::new(design),
    })
    .map_err(|e| e.to_string())?;
    e.end();
    e.doc.validate().map_err(|e| e.to_string())?;
    Ok(e.doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responsive_starters_keep_native_artwork_and_reflow_at_three_widths() {
        for (index, name) in NAMES.iter().enumerate() {
            let doc = create(index, 1200, 900).unwrap();
            let original = doc.clone();
            let root = doc.nodes.iter().find(|n| n.name == *name).unwrap().id;
            let ordered: Vec<_> = doc
                .children(Some(root))
                .into_iter()
                .filter(|id| *id != doc.design.frames[&root].boundary)
                .map(|id| doc.node(id).unwrap().name.as_str())
                .collect();
            assert_eq!(
                ordered,
                [
                    "Eyebrow",
                    "Headline",
                    "Introduction",
                    "Adaptive cards",
                    "Footer"
                ]
            );
            let title = doc.nodes.iter().find(|n| n.name == "Headline").unwrap();
            let footer = doc.nodes.iter().find(|n| n.name == "Footer").unwrap();
            assert!(
                crate::geometry::node_bounds(&doc, title.id).unwrap().y
                    < crate::geometry::node_bounds(&doc, footer.id).unwrap().y
            );

            assert_eq!(doc.design.frames.len(), 5);
            assert!(
                doc.nodes
                    .iter()
                    .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
            );
            for (width, height) in [(375, 2200), (768, 1200), (1440, 1000)] {
                let resized = crate::design_metadata::resize_variant(&doc, width, height).unwrap();
                resized.doc.validate().unwrap();
                let cards = resized
                    .doc
                    .nodes
                    .iter()
                    .find(|n| n.name == "Adaptive cards")
                    .unwrap()
                    .id;
                assert_eq!(
                    layout::effective_frame(&resized.doc, cards).unwrap().flow,
                    if width >= 760 {
                        Flow::Grid
                    } else {
                        Flow::Column
                    }
                );
                assert_eq!(
                    resized
                        .doc
                        .nodes
                        .iter()
                        .filter(|n| matches!(n.kind, NodeKind::Text { .. }))
                        .count(),
                    13
                );
            }
            assert_eq!(doc, original);
        }
        assert_eq!(Template::catalog().count(), 164);
        assert_eq!(Template::Responsive(0).category(), Some(11));
    }
}
