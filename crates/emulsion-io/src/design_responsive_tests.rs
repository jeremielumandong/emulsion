//! End-to-end interchange for clipping and canvas-width layout presets.
use emulsion_core::{
    Command, Document, Editor, Node, NodeId, NodeKind,
    command::Slot,
    design_components::{self as components, Overrides},
    design_layout::{self, Breakpoint, Child, Flow, Frame, FrameOverrides},
    fragment::Fragment,
    project::{Project, ProjectEditor, ProjectKind},
    text::TextSpec,
};
use emulsion_raster::{composite::flatten, vector::PathStyle, vector_geometry::rectangle};
use serde_json::json;
use std::{io::Cursor, sync::Arc};

fn project(doc: &Document) -> Project {
    ProjectEditor::new_project(ProjectKind::Design, doc.clone())
        .unwrap()
        .snapshot()
        .unwrap()
}

fn clip_fixture() -> (Editor, NodeId, Vec<NodeId>) {
    let mut editor = Editor::new(Document::new(240, 160), None);
    let shape = editor
        .execute(Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Overflowing artwork",
                Arc::new(rectangle(20., 20., 160., 60.)),
                PathStyle {
                    fill: Some([220, 20, 40, 255]),
                    stroke: None,
                    ..Default::default()
                },
                240,
                160,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let text = editor
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Editable overflow",
                TextSpec {
                    text: "Native text".into(),
                    x: 80.,
                    y: 50.,
                    size: 20.,
                    ..Default::default()
                },
                240,
                160,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let children = vec![shape, text];
    let group = editor
        .execute(Command::Group {
            ids: children.clone(),
            name: "Clipped card".into(),
        })
        .unwrap()
        .unwrap();
    let mut frame = Frame {
        padding: [0.; 4],
        ..Default::default()
    };
    for child in &children {
        frame.children.insert(
            *child,
            Child {
                absolute: true,
                ..Default::default()
            },
        );
    }
    editor.begin("Create clipping fixture");
    design_layout::enable(&mut editor, group, frame, (80., 80.)).unwrap();
    editor.end();
    let boundary = editor.doc.design.frames[&group].boundary;
    let NodeKind::Path { path, .. } = &editor.doc.node(boundary).unwrap().kind else {
        panic!("Expected rectangular boundary")
    };
    editor
        .execute(Command::SetPath {
            id: boundary,
            path: path.clone(),
            style: PathStyle {
                fill: Some([255; 4]),
                stroke: Some([0, 0, 0, 255]),
                width: 8.,
                ..Default::default()
            },
        })
        .unwrap();
    (editor, group, children)
}

fn clip(editor: &mut Editor, group: NodeId, enabled: bool) {
    let mut design = editor.doc.design.clone();
    let mut frame = serde_json::to_value(&design.frames[&group]).unwrap();
    frame["clip_content"] = json!(enabled);
    design
        .frames
        .insert(group, serde_json::from_value(frame).unwrap());
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .unwrap();
}

fn pixel(bytes: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
    &bytes[(y * width + x) * 4..(y * width + x + 1) * 4]
}

#[test]
fn responsive_clip_keeps_native_sources_border_and_vector_exports() {
    let (mut editor, group, children) = clip_fixture();
    let before = editor.doc.clone();
    clip(&mut editor, group, true);
    for child in &children {
        assert_eq!(editor.doc.node(*child), before.node(*child));
    }
    let rendered = flatten(&editor.doc.composite_tree(), 0).to_srgba8();
    assert_eq!(pixel(&rendered, 240, 50, 40), &[220, 20, 40, 255]);
    assert_eq!(pixel(&rendered, 240, 120, 40)[3], 0);
    // Content clips at x=100, but the boundary's centered stroke survives outside it.
    assert_eq!(pixel(&rendered, 240, 102, 40), &[0, 0, 0, 255]);

    let (svg, flattened) = crate::project_export::svg(&editor.doc).unwrap();
    assert!(!flattened);
    assert!(!String::from_utf8_lossy(&svg).contains("<image"));
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).unwrap();
    let mut bitmap = resvg::tiny_skia::Pixmap::new(240, 160).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut bitmap.as_mut(),
    );
    assert_eq!(pixel(bitmap.data(), 240, 120, 40)[3], 0);
    assert_eq!(pixel(bitmap.data(), 240, 102, 40), &[0, 0, 0, 255]);

    let project = project(&editor.doc);
    let directory = tempfile::tempdir().unwrap();
    let pdf = directory.path().join("clipped-card.pdf");
    let report = crate::project_export::write(
        &project,
        &[project.pages[0].meta.id],
        crate::project_export::Format::Pdf,
        false,
        &pdf,
    )
    .unwrap();
    assert!(report.rasterized_pages.is_empty());
    assert!(std::fs::read(pdf).unwrap().starts_with(b"%PDF-"));
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    let unclipped = flatten(&editor.doc.composite_tree(), 0).to_srgba8();
    assert_eq!(pixel(&unclipped, 240, 120, 40), &[220, 20, 40, 255]);
}

#[test]
fn responsive_clip_project_template_and_clipboard_keep_editable_rules() {
    let (mut editor, group, _) = clip_fixture();
    clip(&mut editor, group, true);
    let project = project(&editor.doc);
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut archive).unwrap();
    let reopened = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    assert_eq!(reopened.pages[0].doc, editor.doc);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("clipped-card.emutemplate");
    crate::template_pack::write(
        &reopened,
        &crate::template_pack::Manifest::new(
            crate::template_pack::Kind::Design,
            "Clipped card".into(),
        ),
        &path,
    )
    .unwrap();
    let pack = crate::template_pack::read(&path).unwrap();
    assert_eq!(pack.project.pages[0].doc, editor.doc);
    let fragment = Fragment::capture(&editor.doc, &[group]).unwrap();
    let original = editor.doc.clone();
    let pasted = fragment.paste(&mut editor, Slot::TOP, (120., 0.)).unwrap()[0];
    assert_ne!(
        editor.doc.design.frames[&pasted].boundary,
        original.design.frames[&group].boundary
    );
    assert_eq!(
        serde_json::to_value(&editor.doc.design.frames[&pasted]).unwrap()["clip_content"],
        true
    );
    let pixels = flatten(&editor.doc.composite_tree(), 0).to_srgba8();
    assert_eq!(pixel(&pixels, 240, 170, 40), &[220, 20, 40, 255]);
    assert_eq!(pixel(&pixels, 240, 230, 40)[3], 0);
    assert!(editor.undo());
    assert_eq!(editor.doc, original);
}

#[test]
fn responsive_breakpoints_roundtrip_and_adapt_to_destination_canvas() {
    let mut editor = Editor::new(Document::new(320, 240), None);
    let children = (0..2)
        .map(|index| {
            editor
                .execute(Command::AddNode {
                    node: Box::new(Node::path(
                        0,
                        "Item",
                        Arc::new(rectangle(10. + index as f64 * 80., 10., 50., 30.)),
                        PathStyle {
                            fill: Some([30, 90, 160, 255]),
                            stroke: None,
                            ..Default::default()
                        },
                        320,
                        240,
                    )),
                    slot: Slot::TOP,
                })
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let group = editor
        .execute(Command::Group {
            ids: children.clone(),
            name: "Adaptive card".into(),
        })
        .unwrap()
        .unwrap();
    let frame = Frame {
        flow: Flow::Column,
        padding: [0.; 4],
        gap: 10.,
        wrap: false,
        breakpoints: vec![
            Breakpoint {
                min_width: 500.,
                overrides: FrameOverrides {
                    flow: Some(Flow::Row),
                    gap: Some(20.),
                    clip_content: Some(true),
                    ..Default::default()
                },
            },
            Breakpoint {
                min_width: 900.,
                overrides: FrameOverrides {
                    columns: Some(3),
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    };
    editor.begin("Adaptive card");
    design_layout::enable(&mut editor, group, frame, (200., 120.)).unwrap();
    editor.end();
    editor
        .execute(Command::Crop {
            rect: emulsion_raster::IRect::new(0, 0, 640, 240),
            rotation: 0.,
        })
        .unwrap();
    let first = emulsion_core::geometry::node_bounds(&editor.doc, children[0])
        .unwrap()
        .unwrap();
    let second = emulsion_core::geometry::node_bounds(&editor.doc, children[1])
        .unwrap()
        .unwrap();
    assert_eq!(first.y, second.y);
    assert_eq!((first.x - second.x).abs(), 70);
    let original = editor.doc.clone();
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&project(&editor.doc), &mut archive).unwrap();
    let reopened = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    assert_eq!(reopened.pages[0].doc, original);
    let mut editor = Editor::new(reopened.pages[0].doc.clone(), None);
    editor
        .execute(Command::Crop {
            rect: emulsion_raster::IRect::new(0, 0, 1000, 240),
            rotation: 0.,
        })
        .unwrap();
    let effective = design_layout::effective_frame(&editor.doc, group).unwrap();
    assert_eq!(effective.flow, Flow::Column);
    assert_eq!(effective.gap, 10.);
    assert!(!effective.clip_content);
    assert_eq!(effective.columns, 3);
    assert!(editor.undo());
    assert_eq!(editor.doc, original);

    // A local reusable fragment adapts to the receiving document's width.
    let fragment = Fragment::capture(&editor.doc, &[group]).unwrap();
    let mut destination = Editor::new(Document::new(1000, 400), None);
    let pasted = fragment
        .paste(&mut destination, Slot::TOP, (40., 40.))
        .unwrap()[0];
    let copied = &destination.doc.design.frames[&pasted];
    assert_eq!(
        copied.breakpoints,
        original.design.frames[&group].breakpoints
    );
    assert_eq!(
        design_layout::effective_frame(&destination.doc, pasted)
            .unwrap()
            .flow,
        Flow::Column
    );
    let content: Vec<_> = destination
        .doc
        .children(Some(pasted))
        .into_iter()
        .filter(|id| *id != copied.boundary)
        .collect();
    let first = emulsion_core::geometry::node_bounds(&destination.doc, content[0])
        .unwrap()
        .unwrap();
    let second = emulsion_core::geometry::node_bounds(&destination.doc, content[1])
        .unwrap()
        .unwrap();
    assert_eq!(first.x, second.x);
    assert_eq!((first.y - second.y).abs(), 40);
    assert!(destination.undo());
    assert!(destination.doc.nodes.is_empty());
}

fn text_member(doc: &Document, root: NodeId) -> NodeId {
    doc.subtree(root)
        .into_iter()
        .find(|id| matches!(doc.node(*id).unwrap().kind, NodeKind::Text { .. }))
        .unwrap()
}

fn set_label(editor: &mut Editor, id: NodeId, text: &str, size: f32) {
    let NodeKind::Text { spec, .. } = &editor.doc.node(id).unwrap().kind else {
        panic!("Expected text")
    };
    let mut spec = (**spec).clone();
    spec.text = text.into();
    spec.size = size;
    editor
        .execute(Command::SetText {
            id,
            spec: Box::new(spec),
        })
        .unwrap();
}

#[test]
fn responsive_components_keep_nested_overrides_after_project_and_template_reopen() {
    let mut editor = Editor::new(Document::new(800, 400), None);
    let text = editor
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Label",
                TextSpec {
                    text: "Shared label".into(),
                    x: 20.,
                    y: 20.,
                    size: 20.,
                    ..Default::default()
                },
                800,
                400,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let inner = components::create(&mut editor, &[text], "Badge").unwrap();
    let outer = editor
        .execute(Command::Group {
            ids: vec![inner],
            name: "Card".into(),
        })
        .unwrap()
        .unwrap();
    components::create(&mut editor, &[outer], "Card").unwrap();
    let copy = components::insert(&mut editor, "Card", "Default", (300., 0.)).unwrap();
    let nested = editor
        .doc
        .subtree(copy)
        .into_iter()
        .find(|id| {
            *id != copy
                && editor
                    .doc
                    .design
                    .component_links
                    .get(id)
                    .is_some_and(|l| l.component == "Badge")
        })
        .unwrap();
    let local_text = text_member(&editor.doc, nested);
    components::set_overrides(
        &mut editor,
        nested,
        local_text,
        Overrides {
            content: true,
            opacity: true,
            ..Default::default()
        },
    )
    .unwrap();
    set_label(&mut editor, local_text, "Personal label", 20.);
    editor
        .execute(Command::SetOpacity {
            id: local_text,
            opacity: 0.4,
        })
        .unwrap();

    let native = project(&editor.doc);
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&native, &mut archive).unwrap();
    let reopened = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    assert_eq!(reopened.pages[0].doc, editor.doc);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested-card.emutemplate");
    crate::template_pack::write(
        &reopened,
        &crate::template_pack::Manifest::new(
            crate::template_pack::Kind::Design,
            "Nested card".into(),
        ),
        &path,
    )
    .unwrap();
    let pack = crate::template_pack::read(&path).unwrap();
    assert_eq!(pack.project.pages[0].doc, editor.doc);
    let mut editor = Editor::new(pack.project.pages[0].doc.clone(), None);
    set_label(&mut editor, text, "Published label", 28.);
    let before = editor.doc.clone();
    components::update(&mut editor, inner, None).unwrap();
    let NodeKind::Text { spec, .. } = &editor.doc.node(local_text).unwrap().kind else {
        panic!("Native text was replaced")
    };
    assert_eq!(spec.text, "Personal label");
    assert_eq!(spec.size, 28.);
    assert_eq!(editor.doc.node(local_text).unwrap().opacity, 0.4);
    assert!(components::overrides_for(&editor.doc, nested, local_text).content);
    assert!(!crate::project_export::svg(&editor.doc).unwrap().1);
    let published = editor.doc.clone();
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(editor.redo());
    assert_eq!(editor.doc, published);

    // The receiving document gets both hidden definitions, with all IDs remapped.
    let fragment = Fragment::capture(&editor.doc, &[copy]).unwrap();
    let mut destination = Editor::new(Document::new(1000, 600), None);
    let pasted = fragment
        .paste(&mut destination, Slot::TOP, (30., 40.))
        .unwrap()[0];
    destination.doc.validate().unwrap();
    assert_eq!(destination.doc.design.components.len(), 2);
    let nested = destination
        .doc
        .subtree(pasted)
        .into_iter()
        .find(|id| {
            *id != pasted
                && destination
                    .doc
                    .design
                    .component_links
                    .get(id)
                    .is_some_and(|l| l.component == "Badge")
        })
        .unwrap();
    let local_text = text_member(&destination.doc, nested);
    assert!(components::overrides_for(&destination.doc, nested, local_text).content);
    components::reset(&mut destination, nested, None).unwrap();
    let NodeKind::Text { spec, .. } = &destination.doc.node(local_text).unwrap().kind else {
        panic!("Reset should preserve member identity")
    };
    assert_eq!(spec.text, "Published label");
    assert_eq!(spec.size, 28.);
    assert_eq!(destination.doc.node(local_text).unwrap().opacity, 1.);
    assert!(components::overrides_for(&destination.doc, nested, local_text).is_empty());
    assert!(destination.undo());
    assert!(components::overrides_for(&destination.doc, nested, local_text).content);
}
