use super::*;
use emulsion_core::{Document, Node, command::Slot, text::TextSpec};
use emulsion_raster::{vector::PathStyle, vector_geometry};
use std::sync::Arc;

fn add(doc: &mut Document, node: Node) -> NodeId {
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(doc)
    .unwrap()
    .unwrap()
}
fn fixture() -> (Editor, NodeId, NodeId, NodeId) {
    let mut doc = Document::new(640, 480);
    let source = add(
        &mut doc,
        Node::text(
            0,
            "Source",
            TextSpec {
                text: "Source".into(),
                size: 36.,
                color: [220, 20, 40, 255],
                bold: true,
                x: 30.,
                y: 25.,
                ..Default::default()
            },
            640,
            480,
        ),
    );
    let text = add(
        &mut doc,
        Node::text(
            0,
            "Target",
            TextSpec {
                text: "Keep this wording".into(),
                size: 18.,
                x: 100.,
                y: 180.,
                rotation: 18.,
                ..Default::default()
            },
            640,
            480,
        ),
    );
    let path = add(
        &mut doc,
        Node::path(
            0,
            "Card",
            Arc::new(vector_geometry::rectangle(350., 80., 120., 60.)),
            PathStyle {
                fill: Some([0, 60, 200, 255]),
                stroke: None,
                ..Default::default()
            },
            640,
            480,
        ),
    );
    (Editor::new(doc, None), source, text, path)
}
fn call(editor: &mut Editor, name: &str, args: Value) -> Value {
    let result = execute(editor, name, &args).expect("registered tool");
    assert!(!result.is_error, "{name}: {:?}", result.content);
    serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
}
fn rejected(editor: &mut Editor, name: &str, args: Value) {
    let before = editor.doc.clone();
    let revision = editor.revision;
    let result = execute(editor, name, &args).unwrap();
    assert!(result.is_error, "{name} accepted {args}");
    assert_eq!(editor.doc, before);
    assert_eq!(editor.revision, revision);
}
#[test]
fn mcp_design_appearance_capture_copy_apply_preserve_content_and_single_undo() {
    let (mut editor, source, text, path) = fixture();
    let before = editor.doc.clone();
    let captured = call(
        &mut editor,
        "describe_object_appearance",
        json!({"node":source}),
    );
    assert!(!editor.undo());
    call(
        &mut editor,
        "copy_object_appearance",
        json!({"source":source,"nodes":[text,path]}),
    );
    let NodeKind::Text { spec, .. } = &editor.doc.node(text).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.text, "Keep this wording");
    assert_eq!((spec.x, spec.y, spec.rotation), (100., 180., 18.));
    assert_eq!(spec.size, 36.);
    assert!(spec.bold);
    let NodeKind::Path {
        path: actual,
        style,
        ..
    } = &editor.doc.node(path).unwrap().kind
    else {
        panic!()
    };
    let NodeKind::Path { path: original, .. } = &before.node(path).unwrap().kind else {
        panic!()
    };
    assert_eq!(actual, original);
    assert_eq!(style.fill, Some([220, 20, 40, 255]));
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(!editor.undo());
    call(
        &mut editor,
        "apply_object_appearance",
        json!({"nodes":[text],"appearance":captured["appearance"]}),
    );
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    let mut bad = captured["appearance"].clone();
    bad["data"]["opacity"] = json!(2.);
    rejected(
        &mut editor,
        "apply_object_appearance",
        json!({"nodes":[text],"appearance":bad}),
    );
    let mut bad = captured["appearance"].clone();
    bad["data"]["invented"] = json!(true);
    rejected(
        &mut editor,
        "apply_object_appearance",
        json!({"nodes":[text],"appearance":bad}),
    );
    rejected(
        &mut editor,
        "apply_object_appearance",
        json!({"nodes":[text],"appearance":{"version":2,"data":captured["appearance"]["data"]}}),
    );
}
#[test]
fn mcp_design_appearance_locked_or_missing_target_prevents_partial_copy() {
    let (mut editor, source, text, path) = fixture();
    editor
        .execute(Command::SetLocked {
            id: path,
            locked: true,
        })
        .unwrap();
    rejected(
        &mut editor,
        "copy_object_appearance",
        json!({"source":source,"nodes":[text,path]}),
    );
    rejected(
        &mut editor,
        "copy_object_appearance",
        json!({"source":source,"nodes":[text,99999]}),
    );
    rejected(
        &mut editor,
        "copy_object_appearance",
        json!({"source":source,"nodes":[text,text]}),
    );
    rejected(
        &mut editor,
        "copy_object_appearance",
        json!({"source":source,"nodes":[]}),
    );
    rejected(
        &mut editor,
        "copy_object_appearance",
        json!({"source":source,"nodes":[text],"surprise":true}),
    );
    editor.begin("User edit");
    rejected(
        &mut editor,
        "copy_object_appearance",
        json!({"source":source,"nodes":[text]}),
    );
    call(
        &mut editor,
        "describe_object_appearance",
        json!({"node":source}),
    );
    assert!(editor.in_transaction());
    editor.cancel();
}
#[test]
fn mcp_design_text_background_create_refit_remove_preserve_native_objects() {
    let (mut editor, _, text, _) = fixture();
    let before = editor.doc.clone();
    rejected(&mut editor, "refit_text_background", json!({"node":text}));
    let created = call(
        &mut editor,
        "set_text_background",
        json!({"node":text,"color":"#12345680","padding":[20,9],"radius":6}),
    );
    let group = created["selected_node"].as_u64().unwrap();
    let bg = created["text_background"]["background"].as_u64().unwrap();
    assert_eq!(
        created["text_background"]["color"],
        json!([18, 52, 86, 128])
    );
    assert_eq!(editor.doc.children(Some(group)), vec![bg, text]);
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(!editor.undo());
    assert!(editor.redo());
    let (x, y, w, _, radius) = formatting::background_geometry(&editor.doc, text, bg).unwrap();
    assert!((radius - 6.).abs() < 0.01);
    let NodeKind::Text { spec, .. } = &editor.doc.node(text).unwrap().kind else {
        panic!()
    };
    let mut spec = (**spec).clone();
    spec.text = "Much longer text requiring a larger backdrop".into();
    editor
        .execute(Command::SetText {
            id: text,
            spec: Box::new(spec),
        })
        .unwrap();
    let edited = editor.doc.clone();
    let fitted = call(&mut editor, "refit_text_background", json!({"node":group}));
    assert_eq!(fitted["selected_node"], group);
    let (fx, fy, fw, _, fr) = formatting::background_geometry(&editor.doc, text, bg).unwrap();
    assert!((fx - x).abs() < 0.01 && (fy - y).abs() < 0.01 && (fr - radius).abs() < 0.01);
    assert!(fw > w);
    assert!(editor.undo());
    assert_eq!(editor.doc, edited);
    assert!(editor.redo());
    let with_background = editor.doc.clone();
    let removed = call(&mut editor, "remove_text_background", json!({"node":text}));
    assert_eq!(removed["selected_node"], text);
    assert!(editor.doc.node(bg).is_none() && editor.doc.node(group).is_none());
    assert!(matches!(
        editor.doc.node(text).unwrap().kind,
        NodeKind::Text { .. }
    ));
    assert!(editor.undo());
    assert_eq!(editor.doc, with_background);
    editor
        .execute(Command::SetLocked {
            id: bg,
            locked: true,
        })
        .unwrap();
    rejected(&mut editor, "refit_text_background", json!({"node":group}));
    rejected(&mut editor, "remove_text_background", json!({"node":text}));
}
#[test]
fn mcp_design_appearance_validates_background_and_corner_geometry_atomically() {
    let (mut editor, _, text, path) = fixture();
    for args in [
        json!({"node":text,"padding":[-1,2]}),
        json!({"node":text,"padding":[1]}),
        json!({"node":text,"padding":[1,2,3]}),
        json!({"node":text,"radius":"6"}),
        json!({"node":text,"radius":10001}),
        json!({"node":text,"color":"#xxff00"}),
    ] {
        rejected(&mut editor, "set_text_background", args);
    }
    let before = editor.doc.clone();
    call(
        &mut editor,
        "set_corner_radius",
        json!({"nodes":[path],"radius":100}),
    );
    let NodeKind::Path { path: shape, .. } = &editor.doc.node(path).unwrap().kind else {
        panic!()
    };
    assert_eq!(formatting::rectangle(shape).unwrap().4, 30.);
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    rejected(
        &mut editor,
        "set_corner_radius",
        json!({"nodes":[path,text],"radius":8}),
    );
    rejected(
        &mut editor,
        "set_corner_radius",
        json!({"nodes":[path],"radius":-1}),
    );
    let ellipse = add(
        &mut editor.doc,
        Node::path(
            0,
            "Circle",
            Arc::new(vector_geometry::ellipse(10., 10., 30., 30.)),
            PathStyle::default(),
            640,
            480,
        ),
    );
    rejected(
        &mut editor,
        "set_corner_radius",
        json!({"nodes":[path,ellipse],"radius":8}),
    );
}
#[test]
fn mcp_design_arrangement_distributes_gaps_and_rejects_locked_group_members() {
    let mut doc = Document::new(400, 240);
    let mut ids = Vec::new();
    for (x, y, w) in [(10., 20., 20.), (90., 50., 40.), (220., 80., 60.)] {
        ids.push(add(
            &mut doc,
            Node::path(
                0,
                "Object",
                Arc::new(vector_geometry::rectangle(x, y, w, 30.)),
                PathStyle {
                    fill: Some([0, 0, 0, 255]),
                    stroke: None,
                    ..Default::default()
                },
                400,
                240,
            ),
        ));
    }
    let mut editor = Editor::new(doc.clone(), None);
    call(
        &mut editor,
        "arrange_nodes",
        json!({"nodes":ids,"operation":"distribute_horizontal_gap"}),
    );
    let bounds: Vec<_> = ids
        .iter()
        .map(|id| emulsion_core::geometry::node_bounds(&editor.doc, *id).unwrap())
        .collect();
    assert_eq!(
        bounds[1].x - bounds[0].x - bounds[0].w,
        bounds[2].x - bounds[1].x - bounds[1].w
    );
    assert!(editor.undo());
    assert_eq!(editor.doc, doc);
    call(
        &mut editor,
        "arrange_nodes",
        json!({"nodes":ids,"operation":"align_top"}),
    );
    assert!(ids.iter().all(|id| {
        emulsion_core::geometry::node_bounds(&editor.doc, *id)
            .unwrap()
            .y
            == 20
    }));
    assert!(editor.undo());
    assert_eq!(editor.doc, doc);
    editor
        .execute(Command::SetLocked {
            id: ids[1],
            locked: true,
        })
        .unwrap();
    rejected(
        &mut editor,
        "arrange_nodes",
        json!({"nodes":ids,"operation":"align_left","target":"canvas"}),
    );
    rejected(
        &mut editor,
        "arrange_nodes",
        json!({"nodes":ids,"operation":"align_left","target":null}),
    );
}
#[test]
fn mcp_design_appearance_definitions_have_closed_schemas_and_dispatch() {
    let (mut editor, _, _, _) = fixture();
    let defs = definitions();
    assert_eq!(defs.len(), 8);
    let names: std::collections::HashSet<_> = defs.iter().map(|d| &d.name).collect();
    assert_eq!(names.len(), defs.len());
    for def in defs {
        assert_eq!(def.input_schema["additionalProperties"], false);
        assert!(
            execute(&mut editor, &def.name, &json!({}))
                .unwrap()
                .is_error
        );
    }
    assert!(execute(&mut editor, "unrelated_tool", &json!({})).is_none());
}
