use super::*;
use crate::{
    Node,
    command::Slot,
    design_layout::{self, Child, Flow, Frame},
    text::TextSpec,
};
fn add(e: &mut Editor, node: Node, parent: Option<NodeId>) -> NodeId {
    e.execute(Command::AddNode {
        node: Box::new(node),
        slot: Slot::top_of(parent),
    })
    .unwrap()
    .unwrap()
}
#[test]
fn responsive_frame_resize_rewraps_nested_text_without_squeezing_glyphs() {
    let mut e = Editor::new(Document::new(800, 600), None);
    let outer = add(&mut e, Node::group(0, "Outer"), None);
    let inner = add(&mut e, Node::group(0, "Inner"), Some(outer));
    let text = add(
        &mut e,
        Node::text(
            0,
            "Heading",
            TextSpec {
                text: "Native text should wrap without compressing letterforms".into(),
                size: 32.,
                width: Some(650.),
                ..Default::default()
            },
            800,
            600,
        ),
        Some(inner),
    );
    design_layout::enable(
        &mut e,
        inner,
        Frame {
            flow: Flow::Column,
            children: BTreeMap::from([(
                text,
                Child {
                    fill_width: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        },
        (700., 250.),
    )
    .unwrap();
    design_layout::enable(
        &mut e,
        outer,
        Frame {
            flow: Flow::Column,
            children: BTreeMap::from([(
                inner,
                Child {
                    fill_width: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        },
        (800., 600.),
    )
    .unwrap();
    e.doc.design.constraints.insert(
        outer,
        Constraint {
            horizontal: Anchor::Stretch,
            vertical: Anchor::Start,
            reflow_text: true,
        },
    );
    let source = e.doc.clone();
    let phone = resize_variant(&source, 360, 600).unwrap().doc;
    let NodeKind::Text { spec: a, .. } = &source.node(text).unwrap().kind else {
        panic!()
    };
    let NodeKind::Text { spec: b, .. } = &phone.node(text).unwrap().kind else {
        panic!()
    };
    assert_eq!(
        (b.size, b.scale_x, b.scale_y, b.rotation),
        (a.size, a.scale_x, a.scale_y, a.rotation)
    );
    assert!(b.width.unwrap() < a.width.unwrap());
    assert!(crate::text::layout(b).bounds().height > crate::text::layout(a).bounds().height);
    assert!((design_layout::bounds(&phone, outer).unwrap().2 - 360.).abs() < 0.01);
    assert_eq!(e.doc, source);
    phone.validate().unwrap();
}
#[test]
fn ordinary_group_reflow_keeps_authored_font_transform() {
    let mut e = Editor::new(Document::new(800, 600), None);
    let group = add(&mut e, Node::group(0, "Group"), None);
    let text = add(
        &mut e,
        Node::text(
            0,
            "Text",
            TextSpec {
                text: "A long editable line of text".into(),
                size: 24.,
                width: Some(400.),
                scale_x: 1.2,
                scale_y: 1.2,
                ..Default::default()
            },
            800,
            600,
        ),
        Some(group),
    );
    e.doc.design.constraints.insert(
        group,
        Constraint {
            horizontal: Anchor::Scale,
            vertical: Anchor::Start,
            reflow_text: true,
        },
    );
    let resized = resize_variant(&e.doc, 400, 600).unwrap().doc;
    let NodeKind::Text { spec, .. } = &resized.node(text).unwrap().kind else {
        panic!()
    };
    assert_eq!((spec.size, spec.scale_x, spec.scale_y), (24., 1.2, 1.2));
    assert!((spec.width.unwrap() - 200.).abs() < 0.01);
}
