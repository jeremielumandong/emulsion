use super::*;
use emulsion_core::{Document, Node, command::Slot};
use emulsion_raster::{Placement, Raster, vector::PathStyle, vector_geometry};
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
fn fixture() -> (Editor, NodeId, Vec<NodeId>) {
    let mut doc = Document::new(640, 480);
    let nodes = (0..3)
        .map(|i| {
            add(
                &mut doc,
                Node::path(
                    0,
                    "Card",
                    Arc::new(vector_geometry::rectangle(
                        20. + f64::from(i) * 90.,
                        30.,
                        60.,
                        40.,
                    )),
                    PathStyle {
                        fill: Some([20, 40, 80, 255]),
                        stroke: None,
                        ..Default::default()
                    },
                    640,
                    480,
                ),
            )
        })
        .collect::<Vec<_>>();
    let group = Command::Group {
        ids: nodes.clone(),
        name: "Cards".into(),
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    (Editor::new(doc, None), group, nodes)
}
fn call(editor: &mut Editor, name: &str, args: Value) -> Value {
    let result = execute(editor, name, &args).unwrap();
    assert!(!result.is_error, "{name}: {:?}", result.content);
    serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
}
fn reject(editor: &mut Editor, name: &str, args: Value) {
    let doc = editor.doc.clone();
    let rev = editor.revision;
    assert!(
        execute(editor, name, &args).unwrap().is_error,
        "Accepted {name}: {args}"
    );
    assert_eq!(editor.doc, doc);
    assert_eq!(editor.revision, rev);
}
#[test]
fn mcp_design_layout_enable_patch_children_remove_and_undo() {
    let (mut editor, group, nodes) = fixture();
    let original = editor.doc.clone();
    call(
        &mut editor,
        "set_responsive_layout",
        json!({"group":group,"flow":"grid","size":[400,240],"padding":[10,12,14,16],"gap":18,"columns":2,"align":"center","wrap":false}),
    );
    let boundary = editor.doc.design.frames[&group].boundary;
    assert_eq!(editor.doc.children(Some(group)).len(), 4);
    assert_eq!(layout::bounds(&editor.doc, group).unwrap().2, 400.);
    assert!(editor.undo());
    assert_eq!(editor.doc, original);
    assert!(!editor.undo());
    assert!(editor.redo());
    let configured = editor.doc.clone();
    call(
        &mut editor,
        "set_layout_child",
        json!({"node":nodes[0],"absolute":true,"fill_width":true}),
    );
    assert!(editor.doc.design.frames[&group].children[&nodes[0]].absolute);
    assert!(editor.undo());
    assert_eq!(editor.doc, configured);
    call(
        &mut editor,
        "set_responsive_layout",
        json!({"group":group,"flow":"column","hug_height":true}),
    );
    assert_eq!(editor.doc.design.frames[&group].gap, 18.);
    assert_eq!(
        editor.doc.design.frames[&group].padding,
        [10., 12., 14., 16.]
    );
    assert_eq!(editor.doc.design.frames[&group].boundary, boundary);
    assert!(editor.undo());
    assert_eq!(editor.doc, configured);
    call(
        &mut editor,
        "remove_responsive_layout",
        json!({"group":group}),
    );
    assert!(editor.doc.design.frames.is_empty());
    assert_eq!(editor.doc.nodes, configured.nodes);
    assert!(editor.undo());
    assert_eq!(editor.doc, configured);
    let info = call(&mut editor, "describe_design_layout", json!({}));
    assert_eq!(info["frames"][group.to_string()]["boundary"], boundary);
}
#[test]
fn mcp_design_layout_invalid_and_locked_reflow_are_atomic() {
    let (mut editor, group, nodes) = fixture();
    for args in [
        json!({"group":group,"columns":0}),
        json!({"group":group,"columns":2.5}),
        json!({"group":group,"flow":"invented"}),
        json!({"group":group,"size":[0,20]}),
        json!({"group":group,"padding":[1,2]}),
        json!({"group":group,"gap":-1}),
        json!({"group":group,"wrap":"yes"}),
    ] {
        reject(&mut editor, "set_responsive_layout", args);
        assert!(editor.doc.design.frames.is_empty());
    }
    editor
        .execute(Command::SetLocked {
            id: nodes[1],
            locked: true,
        })
        .unwrap();
    reject(
        &mut editor,
        "set_responsive_layout",
        json!({"group":group,"flow":"column"}),
    );
    assert!(editor.doc.design.frames.is_empty());
    assert_eq!(editor.doc.children(Some(group)).len(), 3);
    editor.begin("Existing gesture");
    reject(&mut editor, "set_responsive_layout", json!({"group":group}));
    call(&mut editor, "describe_design_layout", json!({}));
    assert!(editor.in_transaction());
    editor.cancel();
}
#[test]
fn mcp_design_constraints_patch_clear_and_preflight_all_targets() {
    let (mut editor, _, nodes) = fixture();
    let before = editor.doc.clone();
    call(
        &mut editor,
        "set_resize_constraints",
        json!({"nodes":nodes,"horizontal":"end","reflow_text":true}),
    );
    assert!(nodes.iter().all(
        |id| editor.doc.design.constraints[id].horizontal == Anchor::End
            && editor.doc.design.constraints[id].reflow_text
    ));
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(!editor.undo());
    assert!(editor.redo());
    let configured = editor.doc.clone();
    call(
        &mut editor,
        "set_resize_constraints",
        json!({"nodes":[nodes[0]],"vertical":"stretch"}),
    );
    assert_eq!(
        editor.doc.design.constraints[&nodes[0]].horizontal,
        Anchor::End
    );
    assert!(editor.undo());
    assert_eq!(editor.doc, configured);
    call(
        &mut editor,
        "clear_resize_constraints",
        json!({"nodes":nodes}),
    );
    assert!(editor.doc.design.constraints.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.doc, configured);
    reject(
        &mut editor,
        "set_resize_constraints",
        json!({"nodes":nodes,"horizontal":"middle"}),
    );
    reject(
        &mut editor,
        "set_resize_constraints",
        json!({"nodes":nodes}),
    );
    editor
        .execute(Command::SetLocked {
            id: nodes[1],
            locked: true,
        })
        .unwrap();
    reject(
        &mut editor,
        "set_resize_constraints",
        json!({"nodes":nodes,"horizontal":"start"}),
    );
    reject(
        &mut editor,
        "clear_resize_constraints",
        json!({"nodes":nodes}),
    );
}
#[test]
fn mcp_design_image_frame_placement_fit_replacement_preserve_source_and_undo() {
    let mut setup = Editor::new(Document::new(640, 480), None);
    let frame = design::frame(&setup.doc, design::Element::Circle)
        .paste(&mut setup, Slot::TOP, (0., 0.))
        .unwrap()[0];
    let boundary = design::frame_parts(&setup.doc, frame).unwrap().0;
    let pixels = Arc::new(Raster::solid(80, 20, [0.2, 0.4, 0.8, 1.]));
    let source = add(
        &mut setup.doc,
        Node::raster(0, "Image", pixels.clone(), Placement::at(5., 6.)),
    );
    let mut editor = Editor::new(setup.doc, None);
    let original = editor.doc.clone();
    let placed = call(
        &mut editor,
        "place_image_in_frame",
        json!({"frame":frame,"source":source}),
    );
    let image = placed["image"].as_u64().unwrap();
    assert_ne!(image, source);
    assert_eq!(editor.doc.node(image).unwrap().clip_to, Some(boundary));
    assert_eq!(editor.doc.node(source), original.node(source));
    let NodeKind::Raster {
        raster,
        placement: cover,
    } = editor.doc.node(image).unwrap().kind.clone()
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(&raster, &pixels));
    assert!(editor.undo());
    assert_eq!(editor.doc, original);
    assert!(!editor.undo());
    assert!(editor.redo());
    let filled = editor.doc.clone();
    call(
        &mut editor,
        "fit_frame_image",
        json!({"node":frame,"fit":"contain","focus":[0.1,0.7]}),
    );
    let NodeKind::Raster { raster, placement } = editor.doc.node(image).unwrap().kind.clone()
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(&raster, &pixels));
    assert!(placement.scale_x < cover.scale_x);
    assert_eq!(editor.doc.node(image).unwrap().clip_to, Some(boundary));
    assert!(editor.undo());
    assert_eq!(editor.doc, filled);
    let replaced = call(
        &mut editor,
        "place_image_in_frame",
        json!({"frame":frame,"source":source}),
    );
    assert_eq!(replaced["image"], image);
    assert_eq!(editor.doc.children(Some(frame)).len(), 2);
    reject(
        &mut editor,
        "fit_frame_image",
        json!({"node":frame,"fit":"cover","focus":[2,0]}),
    );
    reject(
        &mut editor,
        "place_image_in_frame",
        json!({"frame":frame,"source":boundary}),
    );
    editor
        .execute(Command::SetLocked {
            id: image,
            locked: true,
        })
        .unwrap();
    reject(
        &mut editor,
        "place_image_in_frame",
        json!({"frame":frame,"source":source}),
    );
    reject(
        &mut editor,
        "fit_frame_image",
        json!({"node":frame,"fit":"stretch"}),
    );
}
