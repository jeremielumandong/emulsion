//! Typed bindings survive native archives and continue publishing after reopen.
use emulsion_core::{
    Command, Document, Editor, Node, NodeKind,
    command::Slot,
    design_variables::{self as variables, Property, Value},
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
use std::io::Cursor;
#[test]
fn design_variables_archive_roundtrip_retains_bindings_native_text_and_future_updates() {
    let mut e = Editor::new(Document::new(400, 300), None);
    let id = e
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Bound heading",
                TextSpec {
                    text: "Preserved source".into(),
                    underline: true,
                    strikethrough: true,
                    ..Default::default()
                },
                400,
                300,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    variables::set(&mut e, "Brand", Value::Color([30, 90, 180, 255])).unwrap();
    variables::set(&mut e, "Heading", Value::Number(30.)).unwrap();
    variables::bind(&mut e, &[id], Property::TextColor, Some("Brand")).unwrap();
    variables::bind(&mut e, &[id], Property::FontSize, Some("Heading")).unwrap();
    let project = ProjectEditor::new_project(ProjectKind::Design, e.doc.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut archive).unwrap();
    let read = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    assert_eq!(read.pages[0].doc, e.doc);
    let mut reopened = Editor::new(read.pages[0].doc.clone(), None);
    let before = reopened.doc.clone();
    variables::set(&mut reopened, "Brand", Value::Color([190, 30, 60, 255])).unwrap();
    let NodeKind::Text { spec, .. } = &reopened.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.text, "Preserved source");
    assert_eq!(spec.color, [190, 30, 60, 255]);
    assert_eq!(spec.size, 30.);
    assert!(spec.underline && spec.strikethrough);
    let (svg, flattened) = crate::project_export::svg(&reopened.doc).unwrap();
    assert!(!flattened);
    assert!(!String::from_utf8(svg).unwrap().contains("<image"));
    reopened.undo();
    assert_eq!(reopened.doc, before);
}
