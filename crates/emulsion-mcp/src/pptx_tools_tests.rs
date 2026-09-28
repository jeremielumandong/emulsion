use super::*;
#[test]
fn pptx_project_tools_roundtrip_validates_schema_and_keeps_snapshot_unchanged() {
    use emulsion_core::{
        Command, Document, Node,
        command::Slot,
        project::{ProjectEditor, ProjectKind},
        text::TextSpec,
    };
    let mut doc = Document::new(640, 360);
    Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Heading",
            TextSpec {
                text: "Editable via MCP".into(),
                size: 30.,
                width: Some(400.),
                ..Default::default()
            },
            640,
            360,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    let e = ProjectEditor::new_project(ProjectKind::Design, doc).unwrap();
    let project = e.snapshot().unwrap();
    let original = project.pages[0].doc.clone();
    let dir = std::env::temp_dir().join(format!("emulsion-mcp-pptx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("slides.pptx");
    let args = json!({"format":"pptx","path":path.to_str().unwrap()});
    let report = write_snapshot(&project, "export_project", &args).unwrap();
    assert_eq!(report["pages"], 1);
    assert!(report["objects"].as_u64().unwrap() > 0);
    let (imported, warnings) = load_pages(&json!({"source":path.to_str().unwrap()})).unwrap();
    assert!(warnings.is_empty());
    assert!(imported.pages[0].doc.nodes.iter().any(
        |n| matches!(&n.kind,emulsion_core::NodeKind::Text{spec,..}if spec.text=="Editable via MCP")
    ));
    assert_eq!(project.pages[0].doc, original);
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        write_snapshot(
            &project,
            "export_project",
            &json!({"format":"pptx","path":path.to_str().unwrap(),"include_bleed":true})
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(
        validate_args(
            "export_project",
            &json!({"format":"pptx","path":"test.pptx","pages":[0]})
        )
        .is_err()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn web_link_actions_are_strict_atomic_and_readable_over_mcp() {
    use emulsion_core::{Command, Document, Editor, Node, command::Slot};
    let mut e = Editor::new(Document::new(100, 100), None);
    let id = e
        .execute(Command::AddNode {
            node: Box::new(Node::group(0, "Link")),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let before = e.doc.clone();
    let call = |e: &mut Editor, url: &str, trigger: &str| {
        crate::exec::execute(
            e,
            "set_presentation_actions",
            &json!({"node":id,"actions":[{"type":"url","url":url}],"trigger":trigger}),
        )
    };
    assert!(call(&mut e, "javascript:alert(1)", "click").is_error);
    assert!(call(&mut e, "https://example.com", "hover").is_error);
    assert_eq!(e.doc, before);
    assert!(!call(&mut e, "https://example.com", "click").is_error);
    assert!(!crate::exec::execute(&mut e, "get_presentation_actions", &json!({})).is_error);
    e.undo();
    assert_eq!(e.doc, before);
}
