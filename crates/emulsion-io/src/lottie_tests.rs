use super::*;
use emulsion_core::{
    Node, NodeKind,
    design_keyframes::{self, Easing, Keyframe, Property},
};
use serde_json::json;
use std::sync::Arc;
fn fixture() -> Value {
    json!({"v":"5.12.2","w":200,"h":120,"fr":20,"ip":0,"op":20,"layers":[{"ty":4,"ind":1,"nm":"Independent cubic","ip":0,"op":20,"ks":{"a":{"a":0,"k":[0,0]},"p":{"a":1,"k":[{"t":0,"s":[30,20],"e":[90,20],"o":{"x":[0.333],"y":[0.333]},"i":{"x":[0.667],"y":[0.667]}},{"t":20,"s":[90,20]}]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}},"shapes":[{"ty":"sh","nm":"Bezier","ks":{"a":0,"k":{"c":true,"v":[[0,0],[40,0],[20,30]],"i":[[0,0],[-10,-10],[0,0]],"o":[[10,-10],[0,0],[0,0]]}}},{"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100},"r":1}]}]})
}
#[test]
fn lottie_independent_bezier_import_is_editable_animated_and_undoable() {
    let (doc, report) = decode(&serde_json::to_vec(&fixture()).unwrap()).unwrap();
    assert_eq!(report.animated_nodes, 1);
    let path = doc
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Path { .. }))
        .unwrap();
    let NodeKind::Path { path: geometry, .. } = &path.kind else {
        unreachable!()
    };
    assert!(geometry.subpaths[0].anchors[0].has_handles());
    let before = emulsion_core::design_metadata::at_time(&doc, 0).unwrap();
    let after = emulsion_core::design_metadata::at_time(&doc, 1000).unwrap();
    let a = emulsion_core::geometry::node_bounds(&before, path.id).unwrap();
    let b = emulsion_core::geometry::node_bounds(&after, path.id).unwrap();
    assert!((b.x - a.x - 60).abs() <= 1);
    let mut editor = Editor::new(Document::new(200, 120), None);
    let original = editor.doc.clone();
    let ids = insert(&mut editor, &doc).unwrap();
    assert!(!ids.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.doc, original);
    assert!(editor.redo());
    assert!(!editor.doc.design.keyframes.is_empty());
}
#[test]
fn lottie_vector_export_preserves_paths_and_transform_tracks() {
    let mut editor = Editor::new(Document::new(200, 120), None);
    editor.doc.design.duration_ms = 1000;
    editor.doc.design.fps = 20;
    let id = editor.doc.alloc_id();
    editor.doc.nodes.push(Node::path(
        id,
        "Editable square",
        Arc::new(emulsion_raster::vector_geometry::rectangle(
            10., 10., 30., 20.,
        )),
        emulsion_raster::vector::PathStyle {
            fill: Some([255, 30, 20, 255]),
            stroke: None,
            ..Default::default()
        },
        200,
        120,
    ));
    for (time_ms, value) in [(0, 0.), (1000, 80.)] {
        design_keyframes::set_keyframe(
            &mut editor,
            id,
            Property::TranslationX,
            Keyframe {
                time_ms,
                value,
                easing: Easing::EaseInOut,
            },
        )
        .unwrap();
    }
    let original = editor.doc.clone();
    let (bytes, report) = encode(&editor.doc).unwrap();
    std::fs::write(
        std::env::temp_dir().join("emulsion-lottie-editable.json"),
        &bytes,
    )
    .unwrap();
    assert_eq!(report.nodes, 1);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["layers"][0]["ty"], 4);
    assert_eq!(value["layers"][0]["shapes"][0]["ty"], "sh");
    assert_eq!(value["layers"][0]["ks"]["p"]["x"]["a"], 1);
    assert_eq!(value["assets"].as_array().unwrap().len(), 0);
    let (roundtrip, _) = decode(&bytes).unwrap();
    assert!(
        roundtrip
            .nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Path { .. }))
    );
    assert_eq!(editor.doc, original);
    let end = emulsion_core::design_metadata::at_time(&roundtrip, 1000).unwrap();
    let path = end
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Path { .. }))
        .unwrap();
    let b = emulsion_core::geometry::node_bounds(&end, path.id).unwrap();
    assert!((b.x - 88).abs() <= 2, "{b:?}");
}
#[test]
fn lottie_unsupported_operators_are_diagnosed_and_external_assets_not_fetched() {
    let mut value = fixture();
    value["layers"][0]["shapes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"ty":"rp","nm":"Repeater"}));
    let (_, report) = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(report.diagnostics.iter().any(|s| s.contains("rp")));
    value["layers"][0]["parent"] = json!(1);
    assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
    value["layers"][0].as_object_mut().unwrap().remove("parent");
    value["fr"] = json!(0);
    assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
}
#[test]
fn lottie_groups_gradients_text_images_timing_and_compound_holes_roundtrip() {
    use emulsion_raster::vector::{GradientStop, PathPaint, PathStyle};
    let mut doc = Document::new(120, 100);
    doc.design.duration_ms = 1000;
    doc.design.fps = 10;
    let group = doc.alloc_id();
    let shape = doc.alloc_id();
    let mut path = emulsion_raster::vector_geometry::rectangle(10., 10., 80., 60.);
    let mut hole = emulsion_raster::vector_geometry::rectangle(20., 20., 20., 20.);
    hole.subpaths[0].anchors.reverse();
    path.subpaths.extend(hole.subpaths);
    let paint = PathPaint::from_stops(
        &[
            GradientStop {
                offset: 0.,
                color: [255, 0, 0, 255],
            },
            GradientStop {
                offset: 1.,
                color: [0, 0, 255, 128],
            },
        ],
        false,
        0.,
    )
    .unwrap();
    let mut node = Node::path(
        shape,
        "Compound gradient",
        Arc::new(path),
        PathStyle {
            fill: Some([255, 0, 0, 255]),
            fill_paint: paint,
            stroke: None,
            ..Default::default()
        },
        120,
        100,
    );
    node.parent = Some(group);
    doc.nodes.push(node);
    doc.nodes.push(Node::group(group, "Group"));
    let text = doc.alloc_id();
    doc.nodes.push(Node::text(
        text,
        "Editable text",
        emulsion_core::text::TextSpec {
            text: "Hello".into(),
            x: 5.,
            y: 75.,
            size: 12.,
            ..Default::default()
        },
        120,
        100,
    ));
    let image = doc.alloc_id();
    doc.nodes.push(Node::raster(
        image,
        "Embedded image",
        Arc::new(emulsion_raster::Raster::from_srgba8(
            1,
            1,
            &[0, 255, 0, 255],
        )),
        Default::default(),
    ));
    doc.normalize();
    let (bytes, _) = encode(&doc).unwrap();
    let (restored, report) = decode(&bytes).unwrap();
    let path = restored
        .nodes
        .iter()
        .find_map(|n| {
            if let NodeKind::Path { path, style, .. } = &n.kind {
                Some((path, style))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(path.0.subpaths.len(), 2);
    assert_eq!(
        path.1
            .fill_paint
            .gradient_stops(path.1.fill.unwrap())
            .unwrap()[1]
            .color[3],
        128
    );
    assert!(restored.nodes.iter().any(
        |n| matches!(&n.kind,NodeKind::Text{spec,..} if spec.text=="Hello"&&spec.x==5.&&spec.y==75.)
    ));
    assert!(restored.nodes.iter().any(
        |n| matches!(&n.kind,NodeKind::Raster{raster,..} if raster.width()==1&&raster.height()==1)
    ));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("gradient extent"))
    );
    let mut fixture = fixture();
    fixture["layers"][0]["ip"] = json!(5);
    fixture["layers"][0]["op"] = json!(15);
    let (timed, _) = decode(&serde_json::to_vec(&fixture).unwrap()).unwrap();
    let root = timed.nodes.iter().find(|n| n.parent.is_none()).unwrap().id;
    assert!(
        !emulsion_core::design_metadata::at_time(&timed, 0)
            .unwrap()
            .node(root)
            .unwrap()
            .visible
    );
    assert!(
        emulsion_core::design_metadata::at_time(&timed, 500)
            .unwrap()
            .node(root)
            .unwrap()
            .visible
    );
    assert!(
        !emulsion_core::design_metadata::at_time(&timed, 900)
            .unwrap()
            .node(root)
            .unwrap()
            .visible
    );
}
#[test]
fn lottie_native_project_and_vector_export_preserve_source_and_reject_unsupported_effects() {
    let (doc, _) = decode(&serde_json::to_vec(&fixture()).unwrap()).unwrap();
    let source = doc.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("animation.emu");
    let project = emulsion_core::project::ProjectEditor::new_project(
        emulsion_core::project::ProjectKind::Design,
        doc.clone(),
    )
    .unwrap()
    .snapshot()
    .unwrap();
    crate::project::write(&project, &path).unwrap();
    let loaded = crate::project::read(&path).unwrap();
    assert_eq!(loaded.pages[0].doc.design.keyframes, doc.design.keyframes);
    let mut altered = doc.clone();
    altered.design.motion.insert(
        altered.nodes[0].id,
        emulsion_core::design_metadata::Motion {
            end_ms: altered.design.duration_ms,
            transition_ms: 250,
            ..Default::default()
        },
    );
    altered.validate().unwrap();
    let error = encode(&altered).unwrap_err().to_string();
    assert!(
        error.contains("Legacy enter/exit effects") && error.contains("rendered-frame"),
        "{error}"
    );
    assert_eq!(doc, source);
}
