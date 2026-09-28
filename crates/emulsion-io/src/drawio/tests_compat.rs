use super::*;

fn graph(cells: &str) -> String {
    format!(
        r#"<mxGraphModel pageWidth="800" pageHeight="600"><root><mxCell id="0"/><mxCell id="1" parent="0"/>{cells}</root></mxGraphModel>"#
    )
}
fn vertex(id: &str, parent: &str, style: &str, geometry: &str) -> String {
    format!(
        r#"<mxCell id="{id}" value="{id}" vertex="1" parent="{parent}" style="{style}"><mxGeometry {geometry}/></mxCell>"#
    )
}
fn named<'a>(doc: &'a Document, name: &str) -> &'a diagram::Shape {
    doc.diagram
        .as_ref()
        .unwrap()
        .shapes
        .values()
        .find(|s| text(doc, s.label) == name)
        .unwrap()
}
#[test]
fn loose_endpoints_roundtrip_without_fake_visible_shapes() {
    let xml = graph(
        r#"<mxCell id="edge" edge="1" parent="1"><mxGeometry relative="1"><mxPoint as="sourcePoint" x="20" y="40"/><mxPoint as="targetPoint" x="300" y="180"/></mxGeometry></mxCell>"#,
    );
    let imported = from_xml(&xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    let model = doc.diagram.as_ref().unwrap();
    assert_eq!(model.edges.len(), 1);
    assert!(
        model
            .shapes
            .keys()
            .all(|id| !doc.node(*id).unwrap().visible)
    );
    let encoded = to_xml(&imported.project).unwrap();
    assert!(encoded.contains("as=\"sourcePoint\""));
    assert!(!encoded.contains("vertex=\"1\""));
    let round = from_xml(&encoded).unwrap();
    let doc = &round.project.pages[0].doc;
    let edge = doc.diagram.as_ref().unwrap().edges.values().next().unwrap();
    let NodeKind::Path { path, .. } = &doc.node(edge.path).unwrap().kind else {
        panic!()
    };
    assert_eq!(path.subpaths[0].anchors.first().unwrap().p, (20., 40.));
    assert_eq!(path.subpaths[0].anchors.last().unwrap().p, (300., 180.));
}
#[test]
fn relative_children_use_parent_size_and_offset() {
    let cells = vertex(
        "parent",
        "1",
        "group",
        r#"x="100" y="80" width="200" height="100""#,
    ) + r#"<mxCell id="child" value="child" vertex="1" parent="parent"><mxGeometry x="0.5" y="1" width="30" height="20" relative="1"><mxPoint as="offset" x="-15" y="-10"/></mxGeometry></mxCell>"#;
    let imported = from_xml(&graph(&cells)).unwrap();
    let doc = &imported.project.pages[0].doc;
    assert_eq!(
        diagram::shape_bounds(doc, named(doc, "child")),
        Some([185., 170., 30., 20.])
    );
    let parent = named(doc, "parent");
    assert!(
        matches!(&doc.node(parent.body).unwrap().kind,NodeKind::Path {style,..} if style.fill.is_none() && style.stroke.is_none())
    );
}
#[test]
fn hidden_layers_hide_shapes_and_edges() {
    let cells = r#"<mxCell id="hidden" parent="0" visible="0"/>"#.to_string()
        + &vertex("a", "hidden", "", r#"width="80" height="40""#)
        + &vertex("b", "hidden", "", r#"x="200" width="80" height="40""#)
        + r#"<mxCell id="e" parent="hidden" edge="1" source="a" target="b"><mxGeometry relative="1"/></mxCell>"#;
    let imported = from_xml(&graph(&cells)).unwrap();
    let doc = &imported.project.pages[0].doc;
    let model = doc.diagram.as_ref().unwrap();
    assert!(
        model
            .shapes
            .keys()
            .chain(model.edges.keys())
            .all(|id| !doc.node(*id).unwrap().visible)
    );
}
#[test]
fn ellipse_and_text_have_correct_native_geometry_and_paint() {
    let cells = vertex(
        "ellipse",
        "1",
        "ellipse;",
        r#"x="10" y="10" width="100" height="40""#,
    ) + &vertex(
        "text",
        "1",
        "text;html=1;",
        r#"x="200" y="10" width="100" height="40""#,
    );
    let imported = from_xml(&graph(&cells)).unwrap();
    let doc = &imported.project.pages[0].doc;
    let ellipse = named(doc, "ellipse");
    let NodeKind::Path { path, .. } = &doc.node(ellipse.body).unwrap().kind else {
        panic!()
    };
    assert_eq!(path.subpaths[0].anchors.len(), 4);
    let text = named(doc, "text");
    assert!(
        matches!(&doc.node(text.body).unwrap().kind,NodeKind::Path {style,..} if style.fill.is_none() && style.stroke.is_none())
    );
    let round = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    let doc = &round.project.pages[0].doc;
    let ellipse = named(doc, "ellipse");
    assert!(
        matches!(&doc.node(ellipse.body).unwrap().kind,NodeKind::Path {path,..} if path.subpaths[0].anchors.len()==4)
    );
}
#[test]
fn svg_embedded_model_accepts_standard_doctype_and_uri_encoding() {
    let xml = graph(&vertex("a", "1", "", r#"width="80" height="40""#));
    let encoded = xml.bytes().map(|b| format!("%{b:02X}")).collect::<String>();
    let svg = format!(
        r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"><svg content="{encoded}"/>"#
    );
    assert_eq!(from_xml(&svg).unwrap().project.pages.len(), 1);
    assert!(
        from_xml(r#"<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///etc/passwd">]><svg content="&x;"/>"#)
            .is_err()
    );
}
#[test]
fn libraries_open_as_named_editable_pages() {
    let xml = graph(&vertex("a", "1", "", r#"width="80" height="40""#));
    let json =
        serde_json::json!([{"title":"First","xml":xml},{"title":"Second","xml":xml}]).to_string();
    let imported = from_xml(&format!("<mxlibrary>{}</mxlibrary>", escape(&json))).unwrap();
    assert_eq!(imported.project.pages.len(), 2);
    assert_eq!(imported.project.pages[1].meta.name, "Second");
}
#[test]
fn off_page_shapes_expand_canvas_without_changing_relative_positions() {
    let cells = vertex("a", "1", "", r#"x="-40" y="-20" width="80" height="40""#)
        + &vertex("b", "1", "", r#"x="900" y="700" width="80" height="40""#);
    let imported = from_xml(&graph(&cells)).unwrap();
    let doc = &imported.project.pages[0].doc;
    assert!(doc.width >= 1040 && doc.height >= 780);
    let a = diagram::shape_bounds(doc, named(doc, "a")).unwrap();
    let b = diagram::shape_bounds(doc, named(doc, "b")).unwrap();
    assert_eq!((b[0] - a[0], b[1] - a[1]), (940., 720.));
    assert!(a[0] >= 0. && a[1] >= 0.);
}
#[test]
fn outside_ports_remain_attached_and_missing_references_fail() {
    let cells = vertex("a", "1", "", r#"x="100" y="100" width="80" height="40""#)
        + &vertex("b", "1", "", r#"x="300" y="100" width="80" height="40""#)
        + r#"<mxCell id="e" parent="1" edge="1" source="a" target="b" style="exitX=1.25;exitY=0.5;"><mxGeometry/></mxCell>"#;
    let imported = from_xml(&graph(&cells)).unwrap();
    let doc = &imported.project.pages[0].doc;
    let edge = doc.diagram.as_ref().unwrap().edges.values().next().unwrap();
    assert_eq!(edge.source.port, Port::Custom { x: 1.25, y: 0.5 });
    assert!(from_xml(&graph(&cells.replace("target=\"b\"", "target=\"missing\""))).is_err());
}
#[test]
fn embedded_svg_image_is_retained_and_drawio_export_does_not_discard_it() {
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'><rect width='16' height='16' fill='red'/></svg>";
    let data = base64::engine::general_purpose::STANDARD.encode(svg);
    let cells = vertex(
        "a",
        "1",
        &format!("shape=image;image=data:image/svg+xml;base64,{data};"),
        r#"width="16" height="16""#,
    );
    let imported = from_xml(&graph(&cells)).unwrap();
    assert!(imported.project.pages[0].doc.nodes.iter().any(
        |n| matches!(&n.kind, NodeKind::Path { style, .. } if style.fill == Some([255,0,0,255]))
    ));
    let round = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    assert!(round.project.pages[0].doc.nodes.iter().any(
        |n| matches!(&n.kind, NodeKind::Path { style, .. } if style.fill == Some([255,0,0,255]))
    ));
}
#[test]
fn file_detection_distinguishes_editable_svg_from_artwork() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("example.svg");
    std::fs::write(
        &file,
        "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'/>",
    )
    .unwrap();
    assert!(!crate::diagram_import::is_diagram(&file));
    std::fs::write(&file, "<svg content='&lt;mxGraphModel/&gt;'/>").unwrap();
    assert!(crate::diagram_import::is_diagram(&file));
    assert!(crate::diagram_import::is_diagram(
        &dir.path().join("sample.xml")
    ));
}

#[test]
fn moving_and_copying_loose_connectors_keeps_endpoint_handles() {
    let xml = graph(
        r#"<mxCell id="e" edge="1" parent="1"><mxGeometry><mxPoint as="sourcePoint" x="20" y="40"/><mxPoint as="targetPoint" x="200" y="80"/></mxGeometry></mxCell>"#,
    );
    let imported = from_xml(&xml).unwrap();
    let mut doc = imported.project.pages[0].doc.clone();
    let edge_id = *doc.diagram.as_ref().unwrap().edges.keys().next().unwrap();
    let before = doc.clone();
    emulsion_core::transform::transform_nodes(&mut doc, &[edge_id], [1., 0., 0., 1., 30., 50.])
        .unwrap();
    diagram::synchronize(&before, &mut doc).unwrap();
    let model = doc.diagram.as_ref().unwrap();
    let edge = &model.edges[&edge_id];
    let NodeKind::Path { path, .. } = &doc.node(edge.path).unwrap().kind else {
        panic!()
    };
    assert_eq!(path.subpaths[0].anchors[0].p, (50., 90.));
    let fragment = model.fragment(&doc.subtree(edge_id).into_iter().collect());
    assert_eq!(fragment.shapes.len(), 2);
    assert_eq!(fragment.edges.len(), 1);
    doc.validate().unwrap();
}

#[test]
fn drawio_svg_base64_without_marker_and_long_labels_load() {
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><circle cx='4' cy='4' r='4'/></svg>";
    let data = base64::engine::general_purpose::STANDARD.encode(svg);
    let cells = vertex(
        "long",
        "1",
        &format!("shape=image;image=data:image/svg+xml,{data};"),
        r#"width="100" height="40""#,
    )
    .replace("value=\"long\"", &format!("value=\"{}\"", "x".repeat(3000)));
    let imported = from_xml(&graph(&cells)).unwrap();
    let doc = &imported.project.pages[0].doc;
    assert_eq!(
        text(
            doc,
            doc.diagram
                .as_ref()
                .unwrap()
                .shapes
                .values()
                .next()
                .unwrap()
                .label
        )
        .len(),
        3000
    );
    assert!(
        !doc.nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Raster { .. }))
    );
    assert!(
        doc.nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Path { .. }))
            .count()
            >= 2
    );
}

#[test]
fn background_shapes_do_not_cover_connectors_and_stacking_roundtrips() {
    let cells = vertex("background", "1", "ellipse;", r#"width="600" height="400""#)
        + &vertex("a", "1", "", r#"x="40" y="60" width="80" height="40""#)
        + r#"<mxCell id="e" parent="1" edge="1" source="a" target="b"><mxGeometry/></mxCell>"#
        + &vertex("b", "1", "", r#"x="300" y="60" width="80" height="40""#);
    let imported = from_xml(&graph(&cells)).unwrap();
    let round = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    for project in [&imported.project, &round.project] {
        let doc = &project.pages[0].doc;
        let model = doc.diagram.as_ref().unwrap();
        let labels = doc
            .children(None)
            .iter()
            .filter_map(|id| {
                model
                    .shapes
                    .get(id)
                    .map(|s| text(doc, s.label))
                    .or_else(|| model.edges.get(id).map(|_| "edge".into()))
            })
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["background", "a", "edge", "b"]);
    }
}

#[test]
fn bundled_stencils_keep_custom_geometry_in_drawio() {
    for stencil in emulsion_core::diagram::stencils::STENCILS {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let id = stencil
            .insert(&mut editor, [100., 100., 150., 90.])
            .unwrap();
        let project = emulsion_core::project::ProjectEditor::new_project(
            ProjectKind::Diagram,
            editor.doc.clone(),
        )
        .unwrap()
        .snapshot()
        .unwrap();
        let xml = to_xml(&project).unwrap();
        let imported = from_xml(&xml).unwrap_or_else(|e| panic!("{}: {e}", stencil.id));
        let doc = &imported.project.pages[0].doc;
        let shape = doc
            .diagram
            .as_ref()
            .unwrap()
            .shapes
            .values()
            .next()
            .unwrap();
        let original = &editor.doc.diagram.as_ref().unwrap().shapes[&id];
        let NodeKind::Path { path: a, .. } = &editor.doc.node(original.body).unwrap().kind else {
            panic!()
        };
        let NodeKind::Path { path: b, .. } = &doc.node(shape.body).unwrap().kind else {
            panic!()
        };
        assert_eq!(a.subpaths.len(), b.subpaths.len(), "{}", stencil.id);
        assert_eq!(a.anchor_count(), b.anchor_count(), "{}", stencil.id);
        let NodeKind::Text { spec: a, .. } = &editor.doc.node(original.label).unwrap().kind else {
            panic!()
        };
        let NodeKind::Text { spec: b, .. } = &doc.node(shape.label).unwrap().kind else {
            panic!()
        };
        assert!(
            (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001,
            "{}: label position",
            stencil.id
        );
    }
}

#[test]
fn inline_stencil_arcs_ellipses_and_invalid_geometry_are_bounded() {
    fn encoded(xml: &str) -> String {
        let mut writer =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        writer.write_all(xml.as_bytes()).unwrap();
        base64::engine::general_purpose::STANDARD.encode(writer.finish().unwrap())
    }
    let xml = r#"<shape w="100" h="100"><foreground><ellipse x="10" y="10" w="20" h="30"/><path><move x="0" y="0"/><arc rx="100" ry="100" x="100" y="100" sweep-flag="1"/></path><stroke/></foreground></shape>"#;
    let mut warnings = BTreeSet::new();
    let path = stencils::decode(&encoded(xml), [20., 30., 100., 100.], &mut warnings).unwrap();
    assert_eq!(path.subpaths.len(), 2);
    assert!(path.anchor_count() > 4);
    let error = stencils::decode(
        &encoded("<shape w=\"0\"/>"),
        [0., 0., 100., 100.],
        &mut warnings,
    )
    .unwrap_err();
    assert!(error.to_string().contains("Invalid stencil dimensions"));
    let error = stencils::decode(
        &encoded("<!DOCTYPE shape [<!ENTITY x 'test'>]><shape/>"),
        [0., 0., 100., 100.],
        &mut warnings,
    )
    .unwrap_err();
    assert!(error.to_string().contains("entity declarations"));
}

#[test]
fn invalid_inline_decoration_warns_without_losing_valid_graph() {
    let mut writer = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    writer.write_all(br#"<shape w="-Infinity" h="-Infinity"><foreground><path><close/></path></foreground></shape>"#).unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(writer.finish().unwrap());
    let xml = graph(&vertex(
        "icon",
        "1",
        &format!("shape=stencil({encoded});"),
        r#"x="20" y="20" width="100" height="50""#,
    ));
    let imported = from_xml(&xml).unwrap();
    assert_eq!(
        imported.project.pages[0]
            .doc
            .diagram
            .as_ref()
            .unwrap()
            .shapes
            .len(),
        1
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("rectangle placeholder"))
    );
    imported.project.validate().unwrap();
}

#[test]
fn wrapped_labels_use_measured_height_and_connectors_use_drawio_defaults() {
    let cells = vertex(
        "A long paragraph that wraps over several lines inside this shape",
        "1",
        "whiteSpace=wrap;fontSize=16;spacingTop=4;spacingBottom=10;",
        r#"x="40" y="80" width="130" height="150""#,
    ) + &vertex(
        "target",
        "1",
        "",
        r#"x="300" y="80" width="100" height="60""#,
    ) + r#"<mxCell id="edge" edge="1" parent="1" source="A long paragraph that wraps over several lines inside this shape" target="target"><mxGeometry relative="1"/></mxCell>"#;
    let imported = from_xml(&graph(&cells)).unwrap();
    let doc = &imported.project.pages[0].doc;
    let shape = named(
        doc,
        "A long paragraph that wraps over several lines inside this shape",
    );
    let NodeKind::Text { spec, .. } = &doc.node(shape.label).unwrap().kind else {
        panic!()
    };
    let bounds = emulsion_core::text::layout(spec).bounds();
    assert!(bounds.height > spec.size * 2.);
    let expected = 80. + 6. + (150. - 6. - 12. - (bounds.y + bounds.height)) / 2.;
    assert!(
        (spec.y - expected).abs() < 0.01,
        "Measured paragraph is vertically centered"
    );
    let edge = doc.diagram.as_ref().unwrap().edges.values().next().unwrap();
    let NodeKind::Path { style, .. } = &doc.node(edge.path).unwrap().kind else {
        panic!()
    };
    assert_eq!(style.stroke, Some([0, 0, 0, 255]));
    assert_eq!(style.width, 1.);
    assert!(crate::project_export::vector_svg(doc).is_ok());
}

#[test]
fn vendor_geometry_rich_labels_curves_and_markers_remain_editable() {
    let xml = r##"<mxGraphModel pageWidth="640" pageHeight="480"><root><mxCell id="0"/><mxCell id="1" parent="0"/>
    <mxCell id="a" vertex="1" parent="1" value="&lt;b&gt;Server&lt;/b&gt; &lt;span style='color:#ff0000;font-style:italic'&gt;hot&lt;/span&gt;" style="html=1;shape=mxgraph.networks.pc;"><mxGeometry x="20" y="20" width="140" height="100"/></mxCell>
    <mxCell id="b" vertex="1" parent="1" value="Target"><mxGeometry x="380" y="260" width="120" height="80"/></mxCell>
    <mxCell id="e" edge="1" parent="1" source="a" target="b" style="curved=1;startArrow=diamond;startFill=0;endArrow=ERzeroToMany;endSize=12;"><mxGeometry><Array as="points"><mxPoint x="240" y="60"/></Array></mxGeometry></mxCell>
    </root></mxGraphModel>"##;
    let imported = from_xml(xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    let shape = doc
        .diagram
        .as_ref()
        .unwrap()
        .shapes
        .values()
        .find(|s| s.data.contains_key("drawio_vendor_stencil"))
        .unwrap();
    assert!(
        doc.nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Path { .. }))
            .count()
            > 8
    );
    let NodeKind::Text { spec, .. } = &doc.node(shape.label).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.text, "Server hot");
    assert!(spec.runs.iter().any(|r| r.style.bold));
    assert!(
        spec.runs
            .iter()
            .any(|r| r.style.italic && r.style.color == [255, 0, 0, 255])
    );
    let edge = doc.diagram.as_ref().unwrap().edges.values().next().unwrap();
    assert_eq!(edge.routing, Routing::Curved);
    assert_eq!(edge.end_marker.kind, diagram::MarkerKind::ZeroToMany);
    assert!(!edge.start_marker.filled);
    let NodeKind::Path { path, .. } = &doc.node(edge.path).unwrap().kind else {
        panic!()
    };
    assert!(
        path.subpaths[0]
            .anchors
            .iter()
            .any(|a| a.h_in != a.p || a.h_out != a.p)
    );
    let group = *doc
        .diagram
        .as_ref()
        .unwrap()
        .shapes
        .iter()
        .find(|(_, s)| s.label == shape.label)
        .unwrap()
        .0;
    let mut e = Editor::new(doc.clone(), None);
    e.execute(emulsion_core::Command::TranslateNode {
        id: group,
        dx: 30.,
        dy: 20.,
    })
    .unwrap();
    e.doc.validate().unwrap();
    e.undo();
    assert_eq!(e.doc, *doc);
}

#[test]
fn complex_svg_retains_vector_source_through_native_save_and_zoom() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs><circle cx="50" cy="40" r="35" fill="url(#g)"/></svg>"##;
    let doc = crate::svg_vectors::document(svg).unwrap();
    assert!(doc.nodes.iter().any(|n|matches!(&n.kind,NodeKind::Smart{editable:Some(emulsion_core::node::SmartEditable::Svg{xml}),..} if xml.as_ref()==svg)));
    let bytes = crate::project_export::vector_svg(&doc).unwrap();
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("data:image/svg+xml;base64,")
    );
    let scene = crate::svg_viewport::SvgViewport::new(&doc).unwrap();
    assert!(
        scene
            .render((1000, 800), [10., 0., 0., 10., 0., 0.])
            .is_ok()
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.emu");
    crate::ora::write(&doc, &path).unwrap();
    let opened = crate::ora::read_full(&path).unwrap();
    assert!(crate::project_export::vector_svg(&opened.doc).is_ok());
}

#[test]
fn gradient_and_rich_text_survive_drawio_export() {
    let cells = vertex(
        "a",
        "1",
        "fillColor=#112233;gradientColor=#ddeeff;html=1;",
        r#"x="20" y="20" width="180" height="90""#,
    )
    .replace(
        "value=\"a\"",
        "value=\"&lt;b&gt;Bold&lt;/b&gt; &lt;i&gt;Italic&lt;/i&gt;\"",
    );
    let first = from_xml(&graph(&cells)).unwrap();
    let encoded = to_xml(&first.project).unwrap();
    assert!(encoded.contains("data:image/svg+xml"));
    let round = from_xml(&encoded).unwrap();
    let svg =
        String::from_utf8(crate::project_export::vector_svg(&round.project.pages[0].doc).unwrap())
            .unwrap();
    assert!(
        svg.contains("data:image/svg+xml"),
        "Gradient retains SVG source"
    );
}

#[test]
fn bent_connectors_do_not_fill_the_open_path() {
    let imported=from_xml(&graph(&(vertex("a","1","",r#"x="20" y="20" width="80" height="40""#)+&vertex("b","1","",r#"x="200" y="200" width="80" height="40""#)+r#"<mxCell id="e" edge="1" parent="1" source="a" target="b" style="edgeStyle=orthogonalEdgeStyle;"><mxGeometry/></mxCell>"#))).unwrap();
    let doc = &imported.project.pages[0].doc;
    for edge in doc.diagram.as_ref().unwrap().edges.values() {
        let NodeKind::Path { style, .. } = &doc.node(edge.path).unwrap().kind else {
            panic!()
        };
        assert_eq!(style.fill, None);
    }
}

#[test]
fn parameterized_wall_and_vertical_label_keep_orientation() {
    let cells = vertex(
        "wall",
        "1",
        "shape=mxgraph.floorplan.wall;direction=south;fillColor=#000000;",
        r#"x="20" y="20" width="10" height="200""#,
    ) + &vertex(
        "text",
        "1",
        "text;horizontal=0;html=1;",
        r#"x="100" y="20" width="30" height="180""#,
    );
    let imported = from_xml(&graph(&cells)).unwrap();
    let doc = &imported.project.pages[0].doc;
    assert!(!imported.warnings.iter().any(|w| w.contains("rectangle")));
    let wall = named(doc, "wall");
    let artwork = doc
        .nodes
        .iter()
        .find(|n| {
            n.parent == doc.node(wall.body).unwrap().parent
                && n.id != wall.body
                && matches!(n.kind, NodeKind::Path { .. })
        })
        .unwrap();
    let bounds = emulsion_core::geometry::node_bounds(doc, artwork.id).unwrap();
    assert!(
        bounds.h >= 200 && bounds.w <= 14,
        "wall must remain vertical: {bounds:?}"
    );
    let label = named(doc, "text").label;
    let NodeKind::Text { spec, .. } = &doc.node(label).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.rotation, -90.);
    assert!(spec.width.unwrap() > 170.);
    let round = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    let doc = &round.project.pages[0].doc;
    let label = named(doc, "text").label;
    let NodeKind::Text { spec, .. } = &doc.node(label).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.rotation, -90.);
}

#[test]
fn crossing_bridges_rebuild_after_move_and_roundtrip() {
    let input = graph(
        r#"<mxCell id="h" edge="1" parent="1" style="edgeStyle=none;endArrow=none;"><mxGeometry relative="1"><mxPoint x="50" y="150" as="sourcePoint"/><mxPoint x="350" y="150" as="targetPoint"/></mxGeometry></mxCell><mxCell id="v" edge="1" parent="1" style="edgeStyle=none;endArrow=none;jumpStyle=arc;jumpSize=16;"><mxGeometry relative="1"><mxPoint x="200" y="30" as="sourcePoint"/><mxPoint x="200" y="270" as="targetPoint"/></mxGeometry></mxCell>"#,
    );
    let imported = from_xml(&input).unwrap();
    let doc = &imported.project.pages[0].doc;
    let (id, edge) = doc
        .diagram
        .as_ref()
        .unwrap()
        .edges
        .iter()
        .find(|(_, e)| e.jump_style == diagram::JumpStyle::Arc)
        .unwrap();
    let path = |doc: &Document| match &doc.node(edge.path).unwrap().kind {
        NodeKind::Path { path, .. } => path.clone(),
        _ => panic!(),
    };
    assert!(
        path(doc).subpaths[0]
            .anchors
            .iter()
            .any(|a| a.h_in != a.p || a.h_out != a.p)
    );
    let mut editor = Editor::new(doc.clone(), None);
    editor
        .execute(emulsion_core::Command::TranslateNode {
            id: *id,
            dx: 300.,
            dy: 0.,
        })
        .unwrap();
    assert!(
        path(&editor.doc).subpaths[0]
            .anchors
            .iter()
            .all(|a| a.h_in == a.p && a.h_out == a.p)
    );
    editor.undo();
    assert_eq!(&editor.doc, doc);
    let exported = to_xml(&imported.project).unwrap();
    let reopened = from_xml(&exported).unwrap();
    assert!(
        reopened.project.pages[0]
            .doc
            .diagram
            .as_ref()
            .unwrap()
            .edges
            .values()
            .any(|e| e.jump_style == diagram::JumpStyle::Arc && e.jump_size == 16.)
    );
}
#[test]
fn named_layers_links_and_vector_shadows_survive_import_export() {
    let input = graph(&format!(
        r#"<mxCell id="network" value="Network" parent="0"/>{}"#,
        vertex(
            "server",
            "network",
            "shape=cube;shadow=1;",
            "x=\"100\" y=\"80\" width=\"100\" height=\"80\""
        )
        .replace(
            "vertex=\"1\"",
            "vertex=\"1\" link=\"https://example.com/server\""
        )
    ));
    let imported = from_xml(&input).unwrap();
    let doc = &imported.project.pages[0].doc;
    let layer = doc.nodes.iter().find(|n| n.name == "Network").unwrap();
    assert!(layer.is_group());
    assert!(!doc.design.interactions.is_empty());
    let (svg, raster) = crate::project_export::svg(doc).unwrap();
    assert!(!raster);
    assert!(String::from_utf8(svg).unwrap().contains("feOffset"));
    let roundtrip = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    assert!(
        roundtrip.project.pages[0]
            .doc
            .nodes
            .iter()
            .any(|n| n.name == "Network")
    );
    assert!(
        !roundtrip.project.pages[0]
            .doc
            .design
            .interactions
            .is_empty()
    );
}
#[test]
fn large_library_and_page_exceed_previous_caps_without_losing_entries() {
    let mut xml = String::from("<mxfile>");
    for i in 0..101 {
        xml.push_str(&format!(
            "<diagram name=\"Page {i}\">{}</diagram>",
            graph(&vertex("a", "1", "", "width=\"40\" height=\"30\""))
        ));
    }
    xml.push_str("</mxfile>");
    assert_eq!(from_xml(&xml).unwrap().project.pages.len(), 101);
    let cells = (0..1001)
        .map(|i| {
            vertex(
                &format!("s{i}"),
                "1",
                "",
                &format!(
                    "x=\"{}\" y=\"{}\" width=\"20\" height=\"20\"",
                    i % 40 * 30,
                    i / 40 * 30
                ),
            )
        })
        .collect::<String>();
    assert_eq!(
        from_xml(&graph(&cells)).unwrap().project.pages[0]
            .doc
            .diagram
            .as_ref()
            .unwrap()
            .shapes
            .len(),
        1001
    );
}

#[test]
fn extra_connector_labels_follow_routes_and_export_without_losing_text() {
    let xml = graph(
        r#"<mxCell id="edge" edge="1" parent="1" style="endArrow=none;edgeStyle=none;"><mxGeometry relative="1"><mxPoint as="sourcePoint" x="20" y="100"/><mxPoint as="targetPoint" x="400" y="100"/></mxGeometry></mxCell><mxCell id="extra" value="Approved" vertex="1" parent="edge" style="text;html=1;fontSize=14;"><mxGeometry relative="1" x="0.5" y="20"><mxPoint as="offset" x="3" y="4"/></mxGeometry></mxCell>"#,
    );
    let mut imported = from_xml(&xml).unwrap();
    let original = imported.project.pages[0].doc.clone();
    let (id, edge) = original
        .diagram
        .as_ref()
        .unwrap()
        .edges
        .iter()
        .next()
        .unwrap();
    let label = edge.labels[0].node;
    let pos = |d: &Document| match &d.node(label).unwrap().kind {
        NodeKind::Text { spec, .. } => (spec.x, spec.y),
        _ => panic!(),
    };
    let mut editor = Editor::new(original.clone(), None);
    editor
        .execute(emulsion_core::Command::TranslateNode {
            id: *id,
            dx: 35.,
            dy: 17.,
        })
        .unwrap();
    assert_eq!(
        pos(&editor.doc),
        (pos(&original).0 + 35., pos(&original).1 + 17.)
    );
    imported.project.pages[0].doc = editor.doc.clone();
    let reopened = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    let doc = &reopened.project.pages[0].doc;
    let edge = doc.diagram.as_ref().unwrap().edges.values().next().unwrap();
    assert_eq!(edge.labels.len(), 1);
    assert_eq!(text(doc, edge.labels[0].node), "Approved");
    editor.undo();
    assert_eq!(editor.doc, original);
}

#[test]
fn icon_captions_do_not_wrap_unless_requested_and_bpmn_variants_keep_geometry() {
    let xml = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
<mxCell id="2" parent="1" vertex="1" value="Reverse Proxy" style="shape=mxgraph.bpmn.event;symbol=message;verticalLabelPosition=bottom;spacingLeft=86"><mxGeometry x="100" y="100" width="60" height="60"/></mxCell>
<mxCell id="3" parent="1" vertex="1" value="Feature Request?" style="shape=mxgraph.bpmn.gateway2;symbol=none;gwType=exclusive;verticalLabelPosition=top"><mxGeometry x="300" y="100" width="50" height="50"/></mxCell>
</root></mxGraphModel>"#;
    let imported = from_xml(xml).unwrap();
    assert!(
        !imported
            .warnings
            .iter()
            .any(|w| w.contains("editable rectangle")),
        "{:?}",
        imported.warnings
    );
    let doc = &imported.project.pages[0].doc;
    for shape in doc.diagram.as_ref().unwrap().shapes.values() {
        let NodeKind::Text { spec, .. } = &doc.node(shape.label).unwrap().kind else {
            panic!()
        };
        assert!(
            spec.width.unwrap()
                > emulsion_core::diagram::shape_bounds(doc, shape).unwrap()[2] as f32
        );
        let bounds = emulsion_core::text::layout(spec).bounds();
        assert!(
            bounds.height < spec.size * 2.,
            "Caption wrapped: {:?}",
            spec
        );
    }
}

#[test]
fn rounded_connector_and_cyclical_route_roundtrip() {
    let input = graph(
        r#"<mxCell id="e" edge="1" parent="1" style="edgeStyle=none;rounded=1;arcSize=6;strokeWidth=1;endArrow=diamond;endSize=20;dashed=1;dashPattern=8 4 1 4;"><mxGeometry relative="1"><mxPoint x="40" y="60" as="sourcePoint"/><mxPoint x="400" y="240" as="targetPoint"/><Array as="points"><mxPoint x="240" y="60"/><mxPoint x="240" y="240"/></Array></mxGeometry></mxCell>"#,
    );
    let mut imported = from_xml(&input).unwrap();
    let doc = &imported.project.pages[0].doc;
    let edge = doc.diagram.as_ref().unwrap().edges.values().next().unwrap();
    assert_eq!(edge.corner_radius, 6.);
    assert!(
        matches!(&doc.node(edge.path).unwrap().kind,NodeKind::Path{path,..} if path.subpaths[0].anchors.iter().any(|a|a.h_in!=a.p))
    );
    let reopened = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    let edge = reopened.project.pages[0]
        .doc
        .diagram
        .as_ref()
        .unwrap()
        .edges
        .values()
        .next()
        .unwrap();
    assert_eq!(edge.corner_radius, 6.);
    assert_eq!(edge.end_marker.size, 20.);
    let doc = &mut imported.project.pages[0].doc;
    let mut model = doc.diagram.as_deref().unwrap().clone();
    model.edges.values_mut().next().unwrap().routing = Routing::Cyclical;
    doc.diagram = Some(Arc::new(model));
    let reopened = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    assert_eq!(
        reopened.project.pages[0]
            .doc
            .diagram
            .as_ref()
            .unwrap()
            .edges
            .values()
            .next()
            .unwrap()
            .routing,
        Routing::Cyclical
    );
}

#[test]
fn imported_vendor_stencil_reuse_preserves_all_artwork_without_connections() {
    let cells = vertex(
        "pc",
        "1",
        "shape=mxgraph.networks.pc;fillColor=#b2f2eb;",
        r#"x="40" y="50" width="120" height="100""#,
    ) + &vertex(
        "target",
        "1",
        "",
        r#"x="400" y="50" width="120" height="100""#,
    ) + r#"<mxCell id="e" edge="1" parent="1" source="pc" target="target"><mxGeometry/></mxCell>"#;
    let imported = from_xml(&graph(&cells)).unwrap();
    let before = imported.project.pages[0].doc.clone();
    let id = *before
        .diagram
        .as_ref()
        .unwrap()
        .shapes
        .iter()
        .find(|(_, s)| s.data.contains_key("drawio_vendor_stencil"))
        .unwrap()
        .0;
    let original = diagram::document_stencil(&before, id).unwrap();
    assert!(
        original.nodes.len() > 3,
        "compound vendor paths are retained"
    );
    let mut editor = Editor::new(before.clone(), None);
    let ids = diagram::insert_document_stencil(&mut editor, id, (350., 300.)).unwrap();
    let copied = diagram::document_stencil(&editor.doc, ids[0]).unwrap();
    assert_eq!(original.nodes.len(), copied.nodes.len());
    assert_eq!(editor.doc.diagram.as_ref().unwrap().edges.len(), 1);
    assert!(
        copied
            .nodes
            .iter()
            .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
    );
    assert_eq!(
        copied.diagram.as_ref().unwrap().shapes[&ids[0]].data,
        original.diagram.as_ref().unwrap().shapes[&id].data
    );
    editor.doc.validate().unwrap();
    editor.undo();
    assert_eq!(editor.doc, before);
}

#[test]
fn connector_decorations_and_edge_attachments_roundtrip_with_native_artwork() {
    let xml = graph(
        r##"<mxCell id="a" vertex="1" parent="1"><mxGeometry x="10" y="10" width="100" height="50" as="geometry"/></mxCell><mxCell id="b" vertex="1" parent="1"><mxGeometry x="400" y="300" width="100" height="50" as="geometry"/></mxCell><mxCell id="branch" edge="1" source="line" target="b" parent="1" style="exitX=0.25;exitY=0;"/><mxCell id="line" edge="1" source="a" target="b" parent="1" value="Traffic" style="emulsionDoubleLine=1;labelBackgroundColor=#eef4ff;curved=1;jumpStyle=arc;"/>"##,
    );
    let imported = from_xml(&xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    doc.validate().unwrap();
    let model = doc.diagram.as_ref().unwrap();
    let (id, edge) = model.edges.iter().find(|(_, e)| e.double_line).unwrap();
    assert!(edge.double_path.is_some());
    assert!(edge.label_background_path.is_some());
    assert!(model.edges.values().any(|e| e.source.shape == *id));
    let xml = to_xml(&imported.project).unwrap();
    let round = from_xml(&xml).unwrap();
    let model = round.project.pages[0].doc.diagram.as_ref().unwrap();
    assert!(
        model
            .edges
            .values()
            .any(|e| e.double_line && e.label_background == Some([238, 244, 255, 255]))
    );
    assert!(
        model
            .edges
            .values()
            .any(|e| model.edges.contains_key(&e.source.shape))
    );
    let (svg, raster) = crate::project_export::svg(doc).unwrap();
    assert!(!raster);
    assert!(!svg.is_empty());
}

#[test]
fn imported_parameterized_arcs_ribbons_and_stairs_are_not_rectangles() {
    for name in [
        "mxgraph.basic.partConcEllipse",
        "mxgraph.basic.arc",
        "mxgraph.basic.pie",
        "mxgraph.floorplan.stairs",
        "mxgraph.infographic.ribbonSimple",
    ] {
        let xml = graph(&format!(
            r#"<mxCell id="a" vertex="1" parent="1" style="shape={name};startAngle=0.1;endAngle=0.8;notch1=20;notch2=20;"><mxGeometry x="20" y="20" width="140" height="100" as="geometry"/></mxCell>"#
        ));
        let imported = from_xml(&xml).unwrap();
        assert!(
            !imported
                .warnings
                .iter()
                .any(|w| w.contains("rectangle") || w.contains("placeholder")),
            "{name}: {:?}",
            imported.warnings
        );
        let doc = &imported.project.pages[0].doc;
        assert!(
            doc.nodes
                .iter()
                .filter(|n| matches!(&n.kind, NodeKind::Path { .. }))
                .count()
                > 1
        );
    }
}

#[test]
fn html_table_cells_preserve_spans_background_and_editable_text() {
    let value = escape(
        "<table border='1' width='240'><tr><th colspan='2'>Orders</th></tr><tr><td style='background-color:#ffeeaa'>ID</td><td>Customer</td></tr></table>",
    );
    let imported=from_xml(&graph(&format!(r#"<mxCell id="a" vertex="1" parent="1" value="{value}" style="html=1;align=left;verticalAlign=top;"><mxGeometry x="40" y="60" width="240" height="160" as="geometry"/></mxCell>"#))).unwrap();
    let doc = &imported.project.pages[0].doc;
    let text = doc
        .nodes
        .iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Text { spec, .. } if !spec.text.is_empty() => Some(spec),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(text.len(), 3);
    assert_eq!(text[0].text, "Orders");
    assert!(text[0].runs.iter().any(|r| r.style.bold));
    assert!(text[1].x < text[2].x);
    assert_eq!(text[1].y, text[2].y);
    assert!(text[0].width.unwrap() > text[1].width.unwrap());
    assert!(doc.nodes.iter().any(
        |n| matches!(&n.kind,NodeKind::Path{style,..} if style.fill==Some([255,238,170,255]))
    ));
    doc.validate().unwrap();
}

#[test]
fn image_aspect_and_rotation_preserve_native_vector_bounds() {
    use base64::Engine;
    let data=base64::engine::general_purpose::STANDARD.encode(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50" fill="red"/></svg>"#);
    let imported=from_xml(&graph(&format!(r#"<mxCell id="a" vertex="1" parent="1" style="shape=image;imageAspect=1;rotation=90;image=data:image/svg+xml;base64,{data};"><mxGeometry x="100" y="100" width="200" height="200" as="geometry"/></mxCell>"#))).unwrap();
    let doc = &imported.project.pages[0].doc;
    let model = doc.diagram.as_ref().unwrap();
    let shape = model.shapes.values().next().unwrap();
    assert!(
        matches!(&doc.node(shape.body).unwrap().kind,NodeKind::Path{style,..} if style.fill.is_none() && style.stroke.is_none())
    );
    let artwork = doc
        .nodes
        .iter()
        .find(|n| {
            n.id != shape.body
                && matches!(&n.kind,NodeKind::Path{style,..} if style.fill==Some([255,0,0,255]))
        })
        .unwrap();
    let NodeKind::Path { path, .. } = &artwork.kind else {
        panic!()
    };
    let (x, y, w, h) = emulsion_raster::vector_geometry::bounds(path).unwrap();
    assert!(
        (x - 150.).abs() < 0.01
            && (y - 100.).abs() < 0.01
            && (w - 100.).abs() < 0.01
            && (h - 200.).abs() < 0.01,
        "{x},{y},{w},{h}"
    );
}

#[test]
fn primary_connector_labels_keep_route_position_and_normal_on_move_and_roundtrip() {
    let xml = graph(
        r#"<mxCell id="a" vertex="1" parent="1"><mxGeometry x="10" y="10" width="100" height="50" as="geometry"/></mxCell><mxCell id="b" vertex="1" parent="1"><mxGeometry x="10" y="300" width="100" height="50" as="geometry"/></mxCell><mxCell id="e" edge="1" source="a" target="b" parent="1" value="Yes" style="exitX=0.5;exitY=1;entryX=0.5;entryY=0;endArrow=none;"><mxGeometry x="0.5" y="20" relative="1" as="geometry"/></mxCell>"#,
    );
    let imported = from_xml(&xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    let edge = doc.diagram.as_ref().unwrap().edges.values().next().unwrap();
    assert_eq!((edge.label_position, edge.label_normal), (0.5, 20.));
    let NodeKind::Text { spec, .. } = &doc.node(edge.label).unwrap().kind else {
        panic!()
    };
    assert!(
        (spec.x - 20.).abs() < 0.01 && (spec.y - 218.).abs() < 0.01,
        "{},{}",
        spec.x,
        spec.y
    );
    let source = edge.source.shape;
    let label = edge.label;
    let mut editor = Editor::new(doc.clone(), None);
    editor
        .execute(emulsion_core::Command::TranslateNode {
            id: source,
            dx: 0.,
            dy: 40.,
        })
        .unwrap();
    let NodeKind::Text { spec, .. } = &editor.doc.node(label).unwrap().kind else {
        panic!()
    };
    assert!((spec.y - 228.).abs() < 0.01);
    editor.undo();
    assert_eq!(&editor.doc, doc);
    let copy = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
    let edge = copy.project.pages[0]
        .doc
        .diagram
        .as_ref()
        .unwrap()
        .edges
        .values()
        .next()
        .unwrap();
    assert_eq!((edge.label_position, edge.label_normal), (0.5, 20.));
    let mut reversed = edge.clone();
    reversed.reverse();
    assert_eq!(
        (reversed.label_position, reversed.label_normal),
        (-0.5, -20.)
    );
}

#[test]
fn embedded_bitmap_keeps_its_source_position_when_scaled_and_rotated() {
    use base64::Engine;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        100,
        50,
        image::Rgba([255, 0, 0, 255]),
    ))
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let data = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
    let xml = graph(&format!(
        r#"<mxCell id="a" vertex="1" parent="1" style="shape=image;imageAspect=1;rotation=90;image=data:image/png;base64,{data};"><mxGeometry x="100" y="100" width="200" height="200" as="geometry"/></mxCell>"#
    ));
    let imported = from_xml(&xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    let (raster, placement) = doc
        .nodes
        .iter()
        .find_map(|n| match &n.kind {
            NodeKind::Raster { raster, placement } => Some((raster, placement)),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        placement.doc_bounds(raster.width(), raster.height()),
        emulsion_raster::IRect::new(150, 100, 100, 200)
    );
}

#[test]
fn table_css_column_width_padding_height_and_alignment_are_retained() {
    let value = escape(
        "<table width='300' border='1'><colgroup><col width='30%'><col width='70%'></colgroup><tr style='height:80px;background-color:#ffeeaa'><td style='padding:6px 10px;vertical-align:bottom'>ID</td><td style='padding:4px;text-align:right;vertical-align:top'>Name</td></tr></table>",
    );
    let imported=from_xml(&graph(&format!(r#"<mxCell id="a" vertex="1" parent="1" value="{value}" style="html=1;align=left;verticalAlign=top;"><mxGeometry x="40" y="60" width="300" height="160" as="geometry"/></mxCell>"#))).unwrap();
    let doc = &imported.project.pages[0].doc;
    let specs = doc
        .nodes
        .iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Text { spec, .. } if !spec.text.is_empty() => Some(spec),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(specs.len(), 2);
    assert!((specs[0].width.unwrap() - 70.).abs() < 0.1);
    assert!((specs[1].width.unwrap() - 202.).abs() < 0.1);
    assert_eq!(specs[0].align, emulsion_core::text::Align::Left);
    assert_eq!(specs[1].align, emulsion_core::text::Align::Right);
    assert!(specs[0].y > specs[1].y + 20.);
    assert!(doc.nodes.iter().filter(|n|matches!(&n.kind,NodeKind::Path{style,..} if style.fill==Some([255,238,170,255]))).count()>=2);
}

#[test]
fn dynamic_sample_shapes_keep_vector_details_and_editability() {
    for shape in [
        "dimension",
        "mxgraph.mockup.containers.browserWindow",
        "mxgraph.mockup.graphics.simpleIcon",
        "mxgraph.mockup.graphics.iconGrid",
        "mxgraph.mockup.graphics.pieChart",
        "mxgraph.mockup.markup.curlyBrace",
        "mxgraph.bootstrap.topButton",
        "mxgraph.ios7ui.phone",
        "mxgraph.ios7ui.downloadBar",
        "mxgraph.infographic.shadedCube",
        "mxgraph.infographic.bannerSingleFold",
        "mxgraph.arrows2.uTurnArrow",
    ] {
        let xml = graph(&format!(
            r#"<mxCell id="a" vertex="1" parent="1" value="Caption" style="shape={shape};gridSize=4,2;parts=20,30,50;partColors=#ff0000,#00ff00,#0000ff;dy=12;dx=36;dx2=24;notch=15;buttonText=Loading;fillColor=#eeeeee;strokeColor=#333333;"><mxGeometry x="30" y="30" width="180" height="150" as="geometry"/></mxCell>"#
        ));
        let imported = from_xml(&xml).unwrap();
        assert!(
            !imported
                .warnings
                .iter()
                .any(|w| w.contains("editable rectangle")),
            "{shape}: {:?}",
            imported.warnings
        );
        let doc = imported.project.pages[0].doc.clone();
        doc.validate().unwrap();
        assert!(
            doc.nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKind::Path { .. }))
                .count()
                > 1,
            "{shape}"
        );
        assert!(
            !doc.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Raster { .. })),
            "{shape}"
        );
        let id = *doc.diagram.as_ref().unwrap().shapes.keys().next().unwrap();
        let mut editor = emulsion_core::Editor::new(doc.clone(), None);
        editor
            .execute(emulsion_core::Command::TranslateNode {
                id,
                dx: 20.,
                dy: 10.,
            })
            .unwrap();
        editor.undo();
        assert_eq!(editor.doc, doc);
    }
}

#[test]
fn html_fixed_table_keeps_column_widths_height_and_border_style() {
    let value = escape(
        "<table style='table-layout:fixed;width:200px;height:120px;border:2px dashed #123456'><tr><td>Long content that wraps inside a fixed column</td><td>B</td></tr></table>",
    );
    let imported=from_xml(&graph(&format!(r#"<mxCell id="a" vertex="1" parent="1" value="{value}" style="html=1;align=left;verticalAlign=top;"><mxGeometry x="40" y="60" width="200" height="160" as="geometry"/></mxCell>"#))).unwrap();
    let doc = &imported.project.pages[0].doc;
    let cells = doc
        .nodes
        .iter()
        .filter(|n| n.name == "Table cell")
        .collect::<Vec<_>>();
    assert_eq!(cells.len(), 2);
    for node in cells {
        let NodeKind::Path { path, style, .. } = &node.kind else {
            panic!()
        };
        assert_eq!(style.stroke, Some([18, 52, 86, 255]));
        assert_eq!(style.width, 2.);
        assert_eq!(style.dash_count, 2);
        let (_, _, w, h) = emulsion_raster::vector_geometry::bounds(path).unwrap();
        assert_eq!(w, 100.);
        assert!(h >= 120.);
    }
}

#[test]
fn ios_app_bar_imports_status_icons_as_editable_vectors() {
    let imported=from_xml(&graph(r#"<mxCell id="a" vertex="1" parent="1" style="shape=mxgraph.ios7ui.appBar;fillColor2=#222222;"><mxGeometry x="10" y="10" width="300" height="20" as="geometry"/></mxCell>"#)).unwrap();
    assert!(
        !imported
            .warnings
            .iter()
            .any(|w| w.contains("editable rectangle"))
    );
    assert!(
        imported.project.pages[0]
            .doc
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Path { .. }))
            .count()
            > 5
    );
}
