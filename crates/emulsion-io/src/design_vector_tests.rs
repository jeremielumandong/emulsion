use emulsion_core::{Command, Document, Node, NodeKind, command::Slot};
use emulsion_raster::{
    vector::{GradientStop, PathPaint, PathStyle},
    vector_geometry,
};
use std::sync::Arc;
#[test]
fn multistop_native_paints_roundtrip_and_export_as_vectors() {
    let mut doc = Document::new(120, 80);
    let stops = [
        GradientStop {
            offset: 0.,
            color: [255, 0, 0, 255],
        },
        GradientStop {
            offset: 0.4,
            color: [0, 255, 0, 128],
        },
        GradientStop {
            offset: 1.,
            color: [0, 0, 255, 255],
        },
    ];
    for (i, radial) in [false, true].into_iter().enumerate() {
        Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Gradient",
                Arc::new(vector_geometry::rectangle(i as f64 * 60., 0., 60., 80.)),
                PathStyle {
                    fill: Some(stops[0].color),
                    fill_paint: PathPaint::from_stops(&stops, radial, 30.).unwrap(),
                    stroke: None,
                    ..Default::default()
                },
                120,
                80,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gradients.emu");
    crate::ora::write(&doc, &file).unwrap();
    let restored = crate::ora::read(&file).unwrap();
    for node in &doc.nodes {
        let NodeKind::Path { style, .. } = &node.kind else {
            continue;
        };
        let NodeKind::Path { style: actual, .. } = &restored.node(node.id).unwrap().kind else {
            panic!()
        };
        assert_eq!(style, actual);
    }
    let (svg, rasterized) = crate::project_export::svg(&restored).unwrap();
    assert!(!rasterized);
    let svg = String::from_utf8(svg).unwrap();
    assert!(svg.contains("<linearGradient"));
    assert!(svg.contains("<radialGradient"));
    assert!(svg.matches("<stop ").count() >= 6);
    assert!(!svg.contains("data:image/png"));
    assert!(svg.contains("stop-opacity=\"0.5019608\""));
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).unwrap();
    let mut output = resvg::tiny_skia::Pixmap::new(120, 80).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut output.as_mut(),
    );
    assert!(output.pixel(10, 40).unwrap().alpha() > 100);
}

#[test]
fn native_paragraph_lists_roundtrip_and_export_without_flattening() {
    use emulsion_core::text::{ParagraphFormat, ParagraphList, TextSpec, apply_paragraphs};
    let spec = TextSpec {
        text: "A long list item that should wrap in its narrow column\nSecond item".into(),
        size: 18.,
        width: Some(170.),
        height: Some(250.),
        underline: true,
        ..Default::default()
    };
    let spec = apply_paragraphs(
        &spec,
        0..spec.text.len(),
        ParagraphFormat {
            list: ParagraphList::Numbered,
            indent: 36.,
            hanging: 24.,
            space_after: 12.,
            ..Default::default()
        },
    )
    .unwrap();
    let mut doc = Document::new(300, 300);
    let id = Command::AddNode {
        node: Box::new(Node::text(0, "Native list", spec.clone(), 300, 300)),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("list.emu");
    crate::ora::write(&doc, &file).unwrap();
    let opened = crate::ora::read(&file).unwrap();
    let NodeKind::Text { spec: restored, .. } = &opened.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(**restored, spec);
    let (svg, flattened) = crate::project_export::svg(&opened).unwrap();
    assert!(!flattened);
    assert!(!String::from_utf8(svg).unwrap().contains("data:image/png"));
}
