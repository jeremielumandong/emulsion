use emulsion_core::{
    Command, Document, Editor, Node, NodeId,
    command::Slot,
    design_layout::{self, Child, Flow, Frame},
    fragment::Fragment,
    project::{ProjectEditor, ProjectKind},
};
use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
use std::{io::Cursor, sync::Arc};

fn fixture() -> (Editor, NodeId, Vec<NodeId>) {
    let mut editor = Editor::new(Document::new(800, 600), None);
    let children = (0..2)
        .map(|i| {
            editor
                .execute(Command::AddNode {
                    node: Box::new(Node::path(
                        0,
                        format!("Card {i}"),
                        Arc::new(rectangle(i as f64 * 80., 0., 60., 40.)),
                        PathStyle {
                            fill: Some([30, 80, 120, 255]),
                            stroke: None,
                            ..Default::default()
                        },
                        800,
                        600,
                    )),
                    slot: Slot::TOP,
                })
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let group = editor
        .execute(Command::Group {
            ids: children.clone(),
            name: "Responsive cards".into(),
        })
        .unwrap()
        .unwrap();
    let mut frame = Frame {
        flow: Flow::Column,
        padding: [10.; 4],
        gap: 10.,
        min_width: Some(160.),
        max_width: Some(600.),
        min_height: Some(120.),
        max_height: Some(480.),
        ..Default::default()
    };
    frame.children.insert(
        children[0],
        Child {
            fill_width: true,
            min_width: Some(80.),
            max_width: Some(100.),
            aspect_ratio: Some(2.),
            ..Default::default()
        },
    );
    frame.children.insert(
        children[1],
        Child {
            fill_width: true,
            fill_height: true,
            min_width: Some(40.),
            max_width: Some(400.),
            min_height: Some(40.),
            max_height: Some(150.),
            ..Default::default()
        },
    );
    editor.begin("Responsive cards");
    design_layout::enable(&mut editor, group, frame, (320., 240.)).unwrap();
    editor.end();
    (editor, group, children)
}

#[test]
fn responsive_sizing_survives_project_pack_and_native_vector_export() {
    let (editor, group, children) = fixture();
    let project = ProjectEditor::new_project(ProjectKind::Design, editor.doc.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut archive).unwrap();
    let restored = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    assert_eq!(restored.pages[0].doc, editor.doc);

    let directory = tempfile::tempdir().unwrap();
    let pack_path = directory.path().join("cards.emutemplate");
    crate::template_pack::write(
        &restored,
        &crate::template_pack::Manifest::new(crate::template_pack::Kind::Design, "Cards".into()),
        &pack_path,
    )
    .unwrap();
    let pack = crate::template_pack::read(&pack_path).unwrap();
    assert_eq!(pack.project.pages[0].doc, editor.doc);
    let (svg, flattened) = crate::project_export::svg(&pack.project.pages[0].doc).unwrap();
    assert!(!flattened);
    assert!(!String::from_utf8(svg).unwrap().contains("<image"));

    let mut reopened = Editor::new(pack.project.pages[0].doc.clone(), None);
    let before = reopened.doc.clone();
    let frame = reopened.doc.design.frames[&group].clone();
    reopened.begin("Resize reopened frame");
    design_layout::enable(&mut reopened, group, frame, (480., 320.)).unwrap();
    reopened.end();
    let first = emulsion_core::geometry::node_bounds(&reopened.doc, children[0])
        .unwrap()
        .unwrap();
    let second = emulsion_core::geometry::node_bounds(&reopened.doc, children[1])
        .unwrap()
        .unwrap();
    assert_eq!((first.w, first.h), (100, 50));
    assert_eq!((second.w, second.h), (400, 150));
    assert!(reopened.undo());
    assert_eq!(reopened.doc, before);
}

#[test]
fn responsive_sizing_clipboard_remaps_constraints_and_keeps_one_undo() {
    let (mut editor, group, _) = fixture();
    let original = editor.doc.clone();
    let source = original.design.frames[&group].clone();
    let fragment = Fragment::capture(&editor.doc, &[group]).unwrap();
    let pasted = fragment.paste(&mut editor, Slot::TOP, (350., 30.)).unwrap()[0];
    let copied = &editor.doc.design.frames[&pasted];
    assert_ne!(copied.boundary, source.boundary);
    assert_eq!(copied.min_width, source.min_width);
    assert_eq!(copied.max_height, source.max_height);
    assert_eq!(copied.children.len(), source.children.len());
    for (id, rule) in &copied.children {
        assert!(!source.children.contains_key(id));
        assert_eq!(editor.doc.node(*id).unwrap().parent, Some(pasted));
        assert!(source.children.values().any(|value| value == rule));
    }
    editor.doc.validate().unwrap();
    assert!(editor.undo());
    assert_eq!(editor.doc, original);
}
