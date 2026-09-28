use emulsion_core::{
    Command, Document, Editor, Node,
    command::Slot,
    design_styles,
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
use std::io::Cursor;

#[test]
fn named_styles_survive_project_roundtrip_and_vector_export() {
    let mut editor = Editor::new(Document::new(640, 480), None);
    let id = editor
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Title",
                TextSpec {
                    text: "Editable title".into(),
                    ..Default::default()
                },
                640,
                480,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    design_styles::create(&mut editor, id, "Heading").unwrap();
    let project = ProjectEditor::new_project(ProjectKind::Design, editor.doc.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut archive).unwrap();
    let restored = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    assert_eq!(restored.pages[0].doc, editor.doc);
    let (svg, flattened) = crate::project_export::svg(&restored.pages[0].doc).unwrap();
    assert!(!flattened);
    assert!(!String::from_utf8(svg).unwrap().contains("<image"));
    let mut reopened = Editor::new(restored.pages[0].doc.clone(), None);
    reopened
        .execute(Command::SetOpacity { id, opacity: 0.35 })
        .unwrap();
    design_styles::reset(&mut reopened, &[id]).unwrap();
    assert_eq!(reopened.doc.node(id).unwrap().opacity, 1.);
    // Template pages use the same project archive codec; style data is native.
    reopened.doc.validate().unwrap();
}
