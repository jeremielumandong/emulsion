use super::*;
use crate::{Command, Node, command::Slot, fragment::Fragment, text::TextSpec};
fn text(editor: &mut Editor) -> NodeId {
    editor
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Label",
                TextSpec {
                    text: "Editable label".into(),
                    size: 24.,
                    ..Default::default()
                },
                300,
                200,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
}
#[test]
fn variables_materialize_native_properties_rename_unlink_and_undo_atomically() {
    let mut editor = Editor::new(Document::new(300, 200), None);
    let id = text(&mut editor);
    set(&mut editor, "Accent", Value::Color([1, 2, 3, 255])).unwrap();
    bind(&mut editor, &[id], Property::TextColor, Some("Accent")).unwrap();
    set(&mut editor, "Heading", Value::Number(36.)).unwrap();
    bind(&mut editor, &[id], Property::FontSize, Some("Heading")).unwrap();
    let original = editor.doc.clone();
    set(&mut editor, "Heading", Value::Number(52.)).unwrap();
    let NodeKind::Text { spec, cache } = &editor.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.size, 52.);
    assert_eq!(spec.color, [1, 2, 3, 255]);
    assert!(!cache.is_rendered());
    editor.undo();
    assert_eq!(editor.doc, original);
    editor.redo();
    rename(&mut editor, "Heading", "Heading size").unwrap();
    assert_eq!(
        editor.doc.design.variable_bindings[&id][&Property::FontSize],
        "Heading size"
    );
    remove(&mut editor, "Heading size").unwrap();
    assert!(!editor.doc.design.variable_bindings[&id].contains_key(&Property::FontSize));
    let NodeKind::Text { spec, .. } = &editor.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.size, 52.);
}
#[test]
fn variables_reject_wrong_types_locked_consumers_and_invalid_values_without_partial_edits() {
    let mut editor = Editor::new(Document::new(300, 200), None);
    let a = text(&mut editor);
    let b = text(&mut editor);
    set(&mut editor, "Size", Value::Number(24.)).unwrap();
    bind(&mut editor, &[a, b], Property::FontSize, Some("Size")).unwrap();
    let before = editor.doc.clone();
    assert!(set(&mut editor, "Size", Value::Color([0; 4])).is_err());
    assert_eq!(editor.doc, before);
    assert!(set(&mut editor, "Size", Value::Number(f64::NAN)).is_err());
    assert_eq!(editor.doc, before);
    editor.doc.node_mut(b).unwrap().locked = true;
    let before = editor.doc.clone();
    let steps = editor.history.len();
    assert!(set(&mut editor, "Size", Value::Number(30.)).is_err());
    assert_eq!(editor.doc, before);
    assert_eq!(editor.history.len(), steps);
}
#[test]
fn variables_clipboard_collisions_duplicate_and_direct_edits_keep_bindings_consistent() {
    let mut source = Editor::new(Document::new(300, 200), None);
    let id = text(&mut source);
    set(&mut source, "Accent", Value::Color([255, 0, 0, 255])).unwrap();
    bind(&mut source, &[id], Property::TextColor, Some("Accent")).unwrap();
    let fragment = Fragment::capture(&source.doc, &[id]).unwrap();
    let mut target = Editor::new(Document::new(300, 200), None);
    set(&mut target, "Accent", Value::Color([0, 0, 255, 255])).unwrap();
    let ids = fragment.paste(&mut target, Slot::TOP, (0., 0.)).unwrap();
    assert_eq!(
        target.doc.design.variable_bindings[&ids[0]][&Property::TextColor],
        "Accent (2)"
    );
    let copy = target
        .execute(Command::DuplicateNode { id: ids[0] })
        .unwrap()
        .unwrap();
    assert_eq!(
        target.doc.design.variable_bindings[&copy],
        target.doc.design.variable_bindings[&ids[0]]
    );
    let NodeKind::Text { spec, .. } = &target.doc.node(copy).unwrap().kind else {
        panic!()
    };
    let mut spec = (**spec).clone();
    spec.color = [0; 4];
    target
        .execute(Command::SetText {
            id: copy,
            spec: Box::new(spec),
        })
        .unwrap();
    let NodeKind::Text { spec, .. } = &target.doc.node(copy).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.color, [255, 0, 0, 255]);
    target.doc.validate().unwrap();
}
