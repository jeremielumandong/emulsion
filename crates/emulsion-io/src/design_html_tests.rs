use super::*;
use emulsion_core::{
    Command, Document, Editor, Node,
    command::Slot,
    design_components,
    design_interactions::{self, OverlayOperation},
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
fn text(editor: &mut Editor, name: &str, x: f32, y: f32) -> u64 {
    editor
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                name,
                TextSpec {
                    text: name.into(),
                    x,
                    y,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
}
fn fixture() -> (Project, Vec<u64>) {
    let mut e = Editor::new(Document::new(600, 400), None);
    let button = text(&mut e, "Open overlay", 20., 30.);
    let title = text(&mut e, "Overlay content", 100., 150.);
    let overlay = e
        .execute(Command::Group {
            ids: vec![title],
            name: "Overlay".into(),
        })
        .unwrap()
        .unwrap();
    design_interactions::author(&mut e, overlay, vec![Action::CloseOverlay], Some(true)).unwrap();
    design_interactions::author(
        &mut e,
        button,
        vec![Action::Overlay {
            target: overlay,
            operation: OverlayOperation::Toggle,
        }],
        None,
    )
    .unwrap();
    let component_text = text(&mut e, "Component", 250., 250.);
    let component = design_components::create(&mut e, &[component_text], "Card").unwrap();
    design_components::update(&mut e, component, Some("Active")).unwrap();
    design_interactions::author(
        &mut e,
        component,
        vec![Action::Variant {
            target: component,
            variant: "Active".into(),
        }],
        None,
    )
    .unwrap();
    let hover = text(&mut e, "Hover for details", 20., 110.);
    design_interactions::author_with_trigger(
        &mut e,
        hover,
        vec![Action::Overlay {
            target: overlay,
            operation: OverlayOperation::Show,
        }],
        None,
        Some(design_interactions::Trigger::Hover),
    )
    .unwrap();
    let drag = text(&mut e, "Drag to next", 20., 330.);
    design_interactions::author_with_trigger(
        &mut e,
        drag,
        vec![Action::Next],
        None,
        Some(design_interactions::Trigger::DragEnd),
    )
    .unwrap();
    let mut project = ProjectEditor::new_project(ProjectKind::Design, e.doc).unwrap();
    project
        .add_page(
            Document::new(600, 400),
            "Second </script><img src=x onerror=alert(1)>".into(),
            0.,
        )
        .unwrap();
    (
        project.snapshot().unwrap(),
        vec![button, overlay, component],
    )
}
#[test]
fn design_html_native_views_actions_variants_escape_and_preserve_source() {
    let (project, ids) = fixture();
    let original = project.pages[0].doc.clone();
    let (html, report) = build(&project, &[1, 2], &[375, 768]).unwrap();
    assert_eq!(report.pages, 2);
    assert_eq!(report.views, 6);
    assert!(html.contains("data-node=\\\""));
    assert!(!html.contains("<image"));
    assert!(!html.contains("Second </script>"));
    assert!(html.contains("\\u003c/script>"));
    let data = html
        .split("id=\"deck\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let bundle: Value = serde_json::from_str(data).unwrap();
    let deck = &bundle["pages"];
    assert_eq!(deck[0]["views"][0]["width"], 375);
    assert_eq!(
        deck[0]["views"][0]["actions"][ids[0].to_string()][0]["type"],
        "overlay"
    );
    assert!(
        deck[0]["views"][0]["variants"]
            .get(format!("{}:Active", ids[2]))
            .is_some()
    );
    assert_eq!(
        deck[0]["views"][0]["triggers"].as_object().unwrap().len(),
        2
    );
    assert_eq!(project.pages[0].doc, original);
    // A standalone fixture for optional browser acceptance; no external downloads.
    std::fs::write(
        std::env::temp_dir().join("emulsion-design-html-acceptance.html"),
        html,
    )
    .unwrap();
}
#[test]
fn design_html_rejects_unsupported_content_and_invalid_requests_before_file_replace() {
    let (mut project, ids) = fixture();
    assert!(build(&project, &[1, 1], &[]).is_err());
    assert!(build(&project, &[999], &[]).is_err());
    assert!(build(&project, &[1], &[0]).is_err());
    assert!(build(&project, &[1], &[100; 17]).is_err());
    project.pages[0].doc.node_mut(ids[0]).unwrap().blend = emulsion_raster::BlendMode::Multiply;
    let path =
        std::env::temp_dir().join(format!("emulsion-html-atomic-{}.html", std::process::id()));
    std::fs::write(&path, "original").unwrap();
    let error = write(&project, &[1], &[600], &path)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported blending/masks/effects"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn design_html_preserves_visibility_and_directional_transitions_and_rejects_text_reveal() {
    use emulsion_core::{
        design_keyframes::{Easing, Keyframe, Property, Track},
        design_metadata::PageTransition,
    };
    let (mut project, ids) = fixture();
    project.pages[0].doc.design.page_transition = PageTransition::SlideUp;
    project.pages[0].doc.design.keyframes.insert(
        ids[0],
        vec![Track {
            property: Property::Visibility,
            frames: vec![
                Keyframe {
                    time_ms: 0,
                    value: 0.,
                    easing: Easing::Step,
                },
                Keyframe {
                    time_ms: 500,
                    value: 1.,
                    easing: Easing::Linear,
                },
            ],
        }],
    );
    let (html, _) = build(&project, &[1], &[600]).unwrap();
    let encoded = html
        .split("id=\"deck\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let bundle: Value = serde_json::from_str(encoded).unwrap();
    assert_eq!(bundle["pages"][0]["views"][0]["transition"], "slide_up");
    assert_eq!(
        bundle["pages"][0]["views"][0]["keyframes"][ids[0].to_string()][0]["property"],
        "visibility"
    );
    project.pages[0]
        .doc
        .design
        .keyframes
        .get_mut(&ids[0])
        .unwrap()[0]
        .property = Property::TextReveal;
    let error = build(&project, &[1], &[600]).unwrap_err().to_string();
    assert!(error.contains("text-reveal"));
    assert!(error.contains(&format!("object {}", ids[0])));
}
