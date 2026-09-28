use super::ops;
use emulsion_core::{
    Command, Document, Editor, Node, NodeKind,
    command::Slot,
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
use emulsion_raster::{
    vector::{PathPaint, PathStyle},
    vector_geometry,
};
use std::{io::Cursor, sync::Arc};

fn fixture() -> (Document, u64) {
    let mut doc = Document::new(800, 600);
    let id = Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Editable heading",
            TextSpec {
                text: "Clear text".into(),
                font: "Geist".into(),
                size: 42.,
                x: 150.,
                y: 180.,
                rotation: 23.,
                scale_x: 1.2,
                ..Default::default()
            },
            800,
            600,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    (doc, id)
}
fn apply(editor: &mut Editor, commands: Vec<Command>) {
    editor.begin("Appearance");
    for command in commands {
        editor.execute(command).unwrap();
    }
    editor.end();
}
#[test]
fn appearance_background_rotates_with_text_refits_and_roundtrips_as_vectors() {
    let (doc, text) = fixture();
    let mut editor = Editor::new(doc.clone(), None);
    let (commands, group) =
        ops::background(&doc, text, [240, 220, 100, 255], [17., 9.], 7., false).unwrap();
    apply(&mut editor, commands);
    let (_, pair) = ops::text_backdrop(&editor.doc, group).unwrap();
    let (_, background) = pair.unwrap();
    assert_eq!(editor.doc.children(Some(group)), vec![background, text]);
    let (x, y, _, _, radius) = ops::background_geometry(&editor.doc, text, background).unwrap();
    let NodeKind::Text { spec, .. } = &editor.doc.node(text).unwrap().kind else {
        panic!()
    };
    let b = emulsion_core::text::layout(spec).bounds();
    assert!((x - (b.x as f64 - 17.)).abs() < 0.01);
    assert!((y - (b.y as f64 - 9.)).abs() < 0.01);
    assert!((radius - 7.).abs() < 0.01);
    let mut updated = (**spec).clone();
    let styled = editor.doc.clone();
    assert!(editor.undo());
    assert_eq!(editor.doc, doc);
    assert!(editor.redo());
    assert_eq!(editor.doc, styled);
    let project = ProjectEditor::new_project(ProjectKind::Design, styled.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let mut bytes = Cursor::new(Vec::new());
    emulsion_io::project::write_to(&project, &mut bytes).unwrap();
    bytes.set_position(0);
    let restored = emulsion_io::project::read_from(bytes).unwrap();
    assert!(
        ops::text_backdrop(&restored.pages[0].doc, group)
            .unwrap()
            .1
            .is_some()
    );
    let (svg, raster) = emulsion_io::project_export::svg(&restored.pages[0].doc).unwrap();
    assert!(
        !raster,
        "plain text and native backdrop should remain vectors"
    );
    assert!(!String::from_utf8(svg).unwrap().contains("<image"));
    updated.text = "A longer editable heading".into();
    editor
        .execute(Command::SetText {
            id: text,
            spec: Box::new(updated),
        })
        .unwrap();
    let (commands, selection) = ops::background(
        &editor.doc,
        group,
        [100, 200, 255, 255],
        [25., 12.],
        10.,
        false,
    )
    .unwrap();
    assert_eq!(selection, group);
    apply(&mut editor, commands);
    assert_eq!(editor.doc.children(Some(group)), vec![background, text]);
    let (commands, selection) =
        ops::background(&editor.doc, group, [0; 4], [0.; 2], 0., true).unwrap();
    assert_eq!(selection, text);
    apply(&mut editor, commands);
    assert!(editor.doc.node(group).is_none());
    assert!(editor.doc.node(background).is_none());
    assert!(matches!(
        editor.doc.node(text).unwrap().kind,
        NodeKind::Text { .. }
    ));
    assert!(editor.undo());
    assert!(editor.doc.node(background).is_some());
}
#[test]
fn appearance_rounding_preserves_geometry_and_rejects_arbitrary_art() {
    for radius in [0., 3., 20., 999.] {
        let path = ops::rounded_rect(30., 40., 100., 60., radius);
        let (x, y, w, h, r) = ops::rectangle(&path).unwrap();
        assert_eq!((x, y, w, h), (30., 40., 100., 60.));
        assert_eq!(r, radius.min(30.));
    }
    let mut path = vector_geometry::rectangle(0., 0., 100., 100.);
    path.subpaths[0].anchors[1].p.0 = 60.;
    assert!(ops::rectangle(&path).is_none());
    assert!(ops::rectangle(&vector_geometry::ellipse(0., 0., 100., 100.)).is_none());
}
#[test]
fn appearance_gradient_changes_paint_without_destroying_path_or_stroke() {
    let mut doc = Document::new(320, 240);
    let path = Arc::new(ops::rounded_rect(20., 20., 120., 80., 12.));
    let style = PathStyle {
        stroke: Some([240, 0, 0, 255]),
        width: 7.,
        ..Default::default()
    };
    let id = Command::AddNode {
        node: Box::new(Node::path(0, "Card", path.clone(), style, 320, 240)),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    let mut editor = Editor::new(doc.clone(), None);
    let paint = PathPaint::LinearGradient {
        end: [0, 255, 0, 255],
        angle: 37.,
    };
    apply(
        &mut editor,
        ops::fill(&doc, &[id], Some([0, 0, 255, 255]), paint).unwrap(),
    );
    let NodeKind::Path {
        path: actual,
        style: actual_style,
        ..
    } = &editor.doc.node(id).unwrap().kind
    else {
        panic!()
    };
    assert_eq!(actual.as_ref(), path.as_ref());
    assert_eq!(actual_style.fill_paint, paint);
    assert_eq!(actual_style.stroke, style.stroke);
    assert_eq!(actual_style.width, 7.);
    assert!(editor.undo());
    assert_eq!(editor.doc, doc);
    editor
        .execute(Command::SetLocked { id, locked: true })
        .unwrap();
    let command = ops::fill(&editor.doc, &[id], Some([0; 4]), PathPaint::Solid)
        .unwrap()
        .remove(0);
    assert!(editor.execute(command).is_err());
}
#[test]
fn appearance_input_rejects_nonfinite_and_out_of_range_values() {
    for value in ["NaN", "inf", "-inf", "101", "-1", "nonsense"] {
        assert!(ops::number(value, 0., 100.).is_err());
    }
    assert_eq!(ops::number(" 37.5 ", 0., 100.).unwrap(), 37.5);
}
