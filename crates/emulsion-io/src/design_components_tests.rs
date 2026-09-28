use emulsion_core::{
    Command, Document, Editor, Node, NodeKind,
    command::Slot,
    design_components as components,
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
use std::io::Cursor;

#[test]
fn component_sources_variants_and_instances_survive_project_and_stay_out_of_export() {
    let mut e = Editor::new(Document::new(600, 400), None);
    let text = e
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Title",
                TextSpec {
                    text: "Visible title".into(),
                    x: 30.,
                    y: 50.,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let root = components::create(&mut e, &[text], "Title component").unwrap();
    let NodeKind::Text { spec, .. } = &e.doc.node(text).unwrap().kind else {
        panic!()
    };
    let mut spec = (**spec).clone();
    spec.text = "Hidden alternate only".into();
    e.execute(Command::SetText {
        id: text,
        spec: Box::new(spec),
    })
    .unwrap();
    components::update(&mut e, root, Some("Alternate")).unwrap();
    components::reset(&mut e, root, Some("Default")).unwrap();
    let project = ProjectEditor::new_project(ProjectKind::Design, e.doc.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut archive).unwrap();
    let reopened = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    let doc = &reopened.pages[0].doc;
    assert_eq!(doc, &e.doc);
    assert_eq!(doc.design.components["Title component"].variants.len(), 2);
    let (svg, flattened) = crate::project_export::svg(doc).unwrap();
    assert!(!flattened);
    let svg = String::from_utf8(svg).unwrap();
    assert!(
        svg.contains("<path"),
        "editable text exports glyph outlines"
    );
    let mut visible_only = Editor::new(doc.clone(), None);
    for source in components::source_roots(&doc.design) {
        visible_only
            .execute(Command::RemoveNode { id: source })
            .unwrap();
    }
    let (without_sources, _) = crate::project_export::svg(&visible_only.doc).unwrap();
    assert_eq!(
        svg.as_bytes(),
        without_sources.as_slice(),
        "hidden definitions must not contribute any exported artwork"
    );
    let mut reopened = Editor::new(doc.clone(), None);
    components::reset(&mut reopened, root, Some("Alternate")).unwrap();
    assert!(reopened.doc.subtree(root).iter().any(|id|matches!(&reopened.doc.node(*id).unwrap().kind,NodeKind::Text{spec,..} if spec.text=="Hidden alternate only")));
}
