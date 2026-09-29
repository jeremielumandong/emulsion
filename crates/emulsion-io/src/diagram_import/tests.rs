use super::*;
use std::io::Write;
const VDX: &str = r##"<VisioDocument><Pages><Page ID="0" Name="Workflow"><PageSheet><PageWidth>5</PageWidth><PageHeight>4</PageHeight></PageSheet><Shapes>
<Shape ID="1" NameU="Process"><XForm><PinX>1</PinX><PinY>2</PinY><Width>1</Width><Height>0.5</Height></XForm><Text>First &amp; <cp IX="0"/>last</Text><Fill><FillForegnd>#ff0000</FillForegnd></Fill><Geom><MoveTo><X>0</X><Y>0</Y></MoveTo><LineTo><X>1</X><Y>0</Y></LineTo><LineTo><X>1</X><Y>0.5</Y></LineTo><LineTo><X>0</X><Y>0.5</Y></LineTo><LineTo><X>0</X><Y>0</Y></LineTo></Geom></Shape>
<Shape ID="2" NameU="Decision"><XForm><PinX>3</PinX><PinY>2</PinY><Width>1</Width><Height>0.5</Height></XForm><Text>Ready?</Text></Shape>
<Shape ID="3" OneD="1"><XForm1D><BeginX>1.5</BeginX><BeginY>2</BeginY><EndX>2.5</EndX><EndY>2</EndY></XForm1D><Line><EndArrow>1</EndArrow><LineColor>#00ff00</LineColor></Line><Text>Next</Text></Shape>
</Shapes><Connects><Connect FromSheet="3" FromCell="BeginX" ToSheet="1"/><Connect FromSheet="3" FromCell="EndX" ToSheet="2"/></Connects></Page><Page ID="1" Name="Notes"><Shapes/></Page></Pages></VisioDocument>"##;
fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "emulsion-diagram-import-{}-{name}",
        std::process::id()
    ))
}
fn write_zip(path: &Path, parts: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in parts {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}
#[test]
fn visio_xml_import_preserves_all_pages_text_paths_and_bound_lines() {
    let imported = visio::from_xml(VDX).unwrap();
    assert_eq!(imported.project.pages.len(), 2);
    let doc = &imported.project.pages[0].doc;
    doc.validate().unwrap();
    assert_eq!((doc.width, doc.height), (480, 384));
    let model = doc.diagram.as_ref().unwrap();
    assert_eq!(model.shapes.len(), 2);
    assert_eq!(model.edges.len(), 1);
    assert!(
        doc.nodes
            .iter()
            .any(|n| matches!(&n.kind,NodeKind::Text{spec,..} if spec.text=="First & last"))
    );
    let shape = model
        .shapes
        .values()
        .find(|s| s.data.get("import_id").is_some_and(|s| s == "1"))
        .unwrap();
    let NodeKind::Path { path, style, .. } = &doc.node(shape.body).unwrap().kind else {
        panic!()
    };
    assert!(path.subpaths[0].closed);
    assert_eq!(style.fill, Some([255, 0, 0, 255]));
    assert!(model.edges.values().next().unwrap().arrow_end);
    let file = temp("roundtrip.emu");
    crate::project::write(&imported.project, &file).unwrap();
    assert_eq!(crate::project::read(&file).unwrap().pages[0].doc, *doc);
    std::fs::remove_file(file).unwrap();
}
#[test]
fn visio_opc_relationships_master_geometry_and_source_are_retained() {
    let file = temp("master.vsdx");
    write_zip(&file,&[
        ("visio/document.xml",b"<VisioDocument/>"),
        ("visio/pages/pages.xml",br#"<Pages><Page ID="0" Name="Master instance"><PageSheet><Cell N="PageWidth" V="5"/><Cell N="PageHeight" V="4"/></PageSheet><Rel r:id="r1"/></Page></Pages>"#),
        ("visio/pages/_rels/pages.xml.rels",br#"<Relationships><Relationship Id="r1" Target="page1.xml"/></Relationships>"#),
        ("visio/pages/page1.xml",br#"<PageContents><Shapes><Shape ID="1" Master="0"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/></Shape></Shapes></PageContents>"#),
        ("visio/masters/masters.xml",br#"<Masters><Master ID="0" Name="Reusable"><Rel r:id="m1"/></Master></Masters>"#),
        ("visio/masters/_rels/masters.xml.rels",br#"<Relationships><Relationship Id="m1" Target="master1.xml"/></Relationships>"#),
        ("visio/masters/master1.xml",br#"<MasterContents><Shapes><Shape ID="10" NameU="Rectangle"><Cell N="Width" V="1"/><Cell N="Height" V="0.5"/><Text>Master text</Text><Section N="Geometry"><Row T="RelMoveTo"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="RelLineTo"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row><Row T="RelLineTo"><Cell N="X" V="1"/><Cell N="Y" V="1"/></Row><Row T="RelLineTo"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row><Row T="RelLineTo"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section></Shape></Shapes></MasterContents>"#),
    ]);
    let bytes = std::fs::read(&file).unwrap();
    let imported = read(&file).unwrap();
    let doc = &imported.project.pages[0].doc;
    assert!(
        doc.nodes
            .iter()
            .any(|n| matches!(&n.kind,NodeKind::Text{spec,..} if spec.text=="Master text"))
    );
    let s = doc
        .diagram
        .as_ref()
        .unwrap()
        .shapes
        .values()
        .next()
        .unwrap();
    let b = diagram::shape_bounds(doc, s).unwrap();
    assert_eq!(b, [96., 144., 192., 96.]);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    std::fs::remove_file(file).unwrap();
}
fn lucid_document() -> serde_json::Value {
    serde_json::json!({"version":1,"pages":[{"id":"page1","title":"Editable","shapes":[
        {"id":"a","type":"rectangle","boundingBox":{"x":20,"y":20,"w":100,"h":60},"text":"<b>Hello</b>","customData":[{"key":"owner","value":"Local"}]},
        {"id":"b","type":"decision","boundingBox":{"x":220,"y":20,"w":100,"h":60},"text":"Ready?"}],
        "lines":[{"id":"edge","lineType":"straight","endpoint1":{"type":"shapeEndpoint","shapeId":"a","style":"none"},"endpoint2":{"type":"shapeEndpoint","shapeId":"b","style":"arrow"},"text":[{"text":"Next"}]}],
        "groups":[{"id":"group","title":"Workflow","items":["a","b","edge"]}]}]})
}
#[test]
fn lucid_zip_and_json_keep_grouped_editable_diagrams_and_warn_on_html() {
    let text = lucid_document().to_string();
    let file = temp("document.lucid");
    write_zip(&file, &[("document.json", text.as_bytes())]);
    let imported = read(&file).unwrap();
    let doc = &imported.project.pages[0].doc;
    let model = doc.diagram.as_ref().unwrap();
    assert_eq!(model.shapes.len(), 2);
    assert_eq!(model.edges.len(), 1);
    assert!(
        model
            .shapes
            .keys()
            .all(|id| doc.node(*id).unwrap().parent.is_some())
    );
    assert!(
        doc.nodes
            .iter()
            .any(|n| matches!(&n.kind,NodeKind::Text{spec,..} if spec.text=="Hello"))
    );
    assert!(imported.warnings.iter().any(|s| s.contains("HTML")));
    assert_eq!(lucid::from_json(&text).unwrap().project.pages[0].doc, *doc);
    std::fs::remove_file(file).unwrap();
}
#[test]
fn invalid_imports_fail_before_any_project_is_returned() {
    assert!(
        visio::from_xml("<!DOCTYPE x [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><VisioDocument/>")
            .is_err()
    );
    assert!(visio::from_xml(&VDX.replace("ToSheet=\"2\"", "ToSheet=\"999\"")).is_err());
    let mut lucid = lucid_document();
    lucid["pages"][0]["shapes"][1]["id"] = "a".into();
    assert!(lucid::from_json(&lucid.to_string()).is_err());
    let mut lucid = lucid_document();
    lucid["pages"][0]["groups"][0]["items"] = serde_json::json!(["group"]);
    assert!(lucid::from_json(&lucid.to_string()).is_err());
    let file = temp("unsafe.vsdx");
    write_zip(&file, &[("../escape", b"x")]);
    assert!(read(&file).is_err());
    std::fs::remove_file(file).unwrap();
}

#[test]
fn inherited_visio_geometry_scales_absolute_cells_and_local_pin() {
    let text = r#"<VisioDocument><Masters><Master ID="0"><Shapes><Shape ID="10"><XForm><Width>1</Width><Height>1</Height><LocPinX>0.5</LocPinX><LocPinY>0.5</LocPinY></XForm><Geom><MoveTo><X>0</X><Y>0</Y></MoveTo><LineTo><X>1</X><Y>0</Y></LineTo><LineTo><X>1</X><Y>1</Y></LineTo><LineTo><X>0</X><Y>1</Y></LineTo><LineTo><X>0</X><Y>0</Y></LineTo></Geom><Char><Size>0.25</Size><Style>1</Style></Char></Shape></Shapes></Master></Masters><Pages><Page Name="Scaled"><PageSheet><PageWidth>5</PageWidth><PageHeight>5</PageHeight></PageSheet><Shapes><Shape ID="1" Master="0"><XForm><Width>2</Width><Height>2</Height><PinX>2</PinX><PinY>2</PinY></XForm><Text>Scaled</Text></Shape></Shapes></Page></Pages></VisioDocument>"#;
    let imported = visio::from_xml(text).unwrap();
    let doc = &imported.project.pages[0].doc;
    let s = doc
        .diagram
        .as_ref()
        .unwrap()
        .shapes
        .values()
        .next()
        .unwrap();
    assert_eq!(
        diagram::shape_bounds(doc, s).unwrap(),
        [96., 192., 192., 192.]
    );
    let NodeKind::Text { spec, .. } = &doc.node(s.label).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.size, 24.);
    assert!(spec.bold);
}

#[test]
fn recursive_visio_master_is_rejected_without_unbounded_recursion() {
    let text = r#"<VisioDocument><Masters><Master ID="0"><Shapes><Shape ID="10"><Shapes><Shape ID="11" Master="0"/></Shapes></Shape></Shapes></Master></Masters><Pages><Page Name="Cycle"><Shapes><Shape ID="1" Master="0"/></Shapes></Page></Pages></VisioDocument>"#;
    let error = visio::from_xml(text)
        .err()
        .expect("recursive master must fail")
        .to_string();
    assert!(error.contains("nesting"), "{error}");
}

#[test]
fn visio_stencil_uses_masters_instead_of_placeholder_page() {
    let file = temp("placeholder.vssx");
    write_zip(&file,&[
        ("visio/document.xml",b"<VisioDocument/>"),
        ("visio/pages/pages.xml",br#"<Pages><Page ID="0" Name="Placeholder"><PageSheet/></Page></Pages>"#),
        ("visio/masters/masters.xml",br#"<Masters><Master ID="0" Name="Server"><Rel r:id="m1"/></Master></Masters>"#),
        ("visio/masters/_rels/masters.xml.rels",br#"<Relationships><Relationship Id="m1" Target="master1.xml"/></Relationships>"#),
        ("visio/masters/master1.xml",br#"<MasterContents><Shapes><Shape ID="1"><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Text>Server</Text></Shape></Shapes></MasterContents>"#),
    ]);
    let imported = read(&file).unwrap();
    assert_eq!(imported.project.pages.len(), 1);
    assert_eq!(imported.project.pages[0].meta.name, "Server");
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
            .any(|w| w.contains("stencil masters"))
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn renamed_binary_visio_reports_conversion_requirement() {
    let file = temp("renamed.vssx");
    std::fs::write(&file, [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]).unwrap();
    assert!(
        read(&file)
            .err()
            .unwrap()
            .to_string()
            .contains("despite its extension")
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn visio_stencil_suffix_with_drawing_pages_and_themed_cells_imports_natively() {
    let file = temp("pages-not-masters.vssx");
    write_zip(&file, &[
        ("visio/document.xml", b"<VisioDocument/>"),
        ("visio/pages/pages.xml", br#"<Pages><Page ID="0" Name="Rack"><PageSheet><Cell N="PageWidth" V="5"/><Cell N="PageHeight" V="4"/></PageSheet><Rel r:id="r1"/></Page></Pages>"#),
        ("visio/pages/_rels/pages.xml.rels", br#"<Relationships><Relationship Id="r1" Target="page1.xml"/></Relationships>"#),
        ("visio/pages/page1.xml", br#"<PageContents><Shapes><Shape ID="1"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LineWeight" V="Themed"/><Text>Network switch</Text></Shape></Shapes></PageContents>"#),
    ]);
    let imported = visio::package(&file).unwrap();
    assert_eq!(imported.project.pages.len(), 1);
    assert_eq!(imported.project.pages[0].meta.name, "Rack");
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
            .any(|w| w.contains("instead of masters"))
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn visio_group_labels_text_frames_and_geometry_visibility_are_preserved() {
    let xml = r##"<VisioDocument><Pages><Page><PageSheet><PageWidth>5</PageWidth><PageHeight>4</PageHeight></PageSheet><Shapes>
<Shape ID="1" Type="Group"><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>
<Cell N="TxtWidth" V="1.5"/><Cell N="TxtHeight" V="0.25"/><Cell N="TxtPinX" V="1"/><Cell N="TxtPinY" V="0.8"/><Cell N="TxtLocPinX" V="0.75"/><Cell N="TxtLocPinY" V="0.125"/><Cell N="VerticalAlign" V="0"/>
<Section N="Character"><Row IX="0"><Cell N="Size" V="0.125"/><Cell N="Style" V="1"/></Row><Row IX="1"><Cell N="Size" V="0.125"/><Cell N="Style" V="0"/></Row></Section>
<Text><cp IX="0"/>Title <cp IX="1"/>value</Text><Shapes>
<Shape ID="2"><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="PinX" V="1"/><Cell N="PinY" V="0.5"/>
<Section N="Geometry"><Cell N="NoFill" V="1"/><Cell N="NoLine" V="1"/><Row T="RelMoveTo"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="RelLineTo"><Cell N="X" V="1"/><Cell N="Y" V="1"/></Row></Section>
<Section N="Geometry" IX="1"><Cell N="NoFill" V="1"/><Row T="RelMoveTo"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="RelLineTo"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row></Section></Shape>
<Shape ID="3"><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Text>Text only</Text></Shape>
</Shapes></Shape></Shapes></Page></Pages></VisioDocument>"##;
    let imported = visio::from_xml(xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    let model = doc.diagram.as_ref().unwrap();
    let lookup = |key: &str| {
        model
            .shapes
            .values()
            .find(|s| s.data.get("import_id").map(String::as_str) == Some(key))
            .unwrap()
    };
    let parent = lookup("1");
    let child = lookup("2");
    let text_only = lookup("3");
    let index = |id| doc.nodes.iter().position(|n| n.id == id).unwrap();
    assert!(index(parent.label) > index(child.body));
    for id in [child.body, text_only.body] {
        let NodeKind::Path { style, .. } = &doc.node(id).unwrap().kind else {
            panic!()
        };
        assert!(style.fill.is_none() && style.stroke.is_none());
    }
    assert!(doc.nodes.iter().any(|n|n.parent==doc.node(child.body).unwrap().parent && matches!(&n.kind,NodeKind::Path{style,..} if style.fill.is_none()&&style.stroke.is_some())));
    let NodeKind::Text { spec, .. } = &doc.node(parent.label).unwrap().kind else {
        panic!()
    };
    assert!(
        (spec.x - 120.).abs() < 0.01 && (spec.y - 151.2).abs() < 0.01,
        "{spec:?}"
    );
    assert_eq!(spec.width, Some(144.));
    assert_eq!(spec.text, "Title value");
    assert!(spec.bold && !spec.runs[0].style.bold);
    assert_eq!((spec.runs[0].start, spec.runs[0].end), (6, 11));
    let mut moved = doc.clone();
    let before = spec.clone();
    emulsion_core::command::Command::TranslateNode {
        id: doc.node(parent.body).unwrap().parent.unwrap(),
        dx: 20.,
        dy: 30.,
    }
    .apply(&mut moved)
    .unwrap();
    let NodeKind::Text { spec, .. } = &moved.node(parent.label).unwrap().kind else {
        panic!()
    };
    assert_eq!((spec.x, spec.y), (before.x + 20., before.y + 30.));
}

#[test]
fn visio_embedded_master_bitmap_uses_scaled_placement_and_native_clip() {
    let png = crate::export::png8(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
    let file = temp("bitmap.vsdx");
    write_zip(&file,&[
        ("visio/document.xml",b"<VisioDocument/>"),
        ("visio/pages/pages.xml",br#"<Pages><Page ID="0"><PageSheet><Cell N="PageWidth" V="2"/><Cell N="PageHeight" V="2"/></PageSheet><Rel r:id="r1"/></Page></Pages>"#),
        ("visio/pages/_rels/pages.xml.rels",br#"<Relationships><Relationship Id="r1" Target="page1.xml"/></Relationships>"#),
        ("visio/pages/page1.xml",br#"<PageContents><Shapes><Shape ID="1" Master="0"><Cell N="PinX" V="0.5"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/></Shape></Shapes></PageContents>"#),
        ("visio/masters/masters.xml",br#"<Masters><Master ID="0"><Rel r:id="m1"/></Master></Masters>"#),
        ("visio/masters/_rels/masters.xml.rels",br#"<Relationships><Relationship Id="m1" Target="master1.xml"/></Relationships>"#),
        ("visio/masters/master1.xml",br#"<MasterContents><Shapes><Shape ID="10"><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="ImgWidth" V="2"/><Cell N="ImgHeight" V="1"/><Cell N="ImgOffsetX" V="-0.5"/><ForeignData ForeignType="Bitmap"><Rel r:id="image"/></ForeignData></Shape></Shapes></MasterContents>"#),
        ("visio/masters/_rels/master1.xml.rels",br#"<Relationships><Relationship Id="image" Target="../media/image.png"/></Relationships>"#),
        ("visio/media/image.png",&png),
    ]);
    let imported = read(&file).unwrap();
    let doc = &imported.project.pages[0].doc;
    let node = doc
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Raster { .. }))
        .unwrap();
    let NodeKind::Raster { raster, placement } = &node.kind else {
        panic!()
    };
    let matrix = placement.to_doc(raster.width(), raster.height());
    assert_eq!(
        matrix.transform_point2(glam::dvec2(1., 0.5)),
        glam::dvec2(48., 144.)
    );
    assert!(node.clip_to.is_some());
    let (svg, fallback) = crate::project_export::svg(doc).unwrap();
    assert!(!fallback);
    assert!(
        String::from_utf8(svg)
            .unwrap()
            .contains("data:image/png;base64,")
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn visio_loose_and_partial_lines_are_reconnectable_and_roundtrip() {
    let xml = outlined_target().replace(
        "<Connect FromSheet=\"3\" FromCell=\"EndX\" ToSheet=\"2\"/>",
        "",
    );
    let imported = visio::from_xml(&xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    let model = doc.diagram.as_ref().unwrap();
    assert_eq!(model.edges.len(), 1);
    let edge = model.edges.values().next().unwrap();
    assert_eq!(model.shapes[&edge.source.shape].data["import_id"], "1");
    assert_eq!(model.shapes[&edge.target.shape].data["import_id"], "2");
    assert!(edge.arrow_end);
    let before = diagram::endpoint_position(doc, &edge.source, (0., 0.)).unwrap();
    let mut editor = emulsion_core::Editor::new(doc.clone(), None);
    editor
        .execute(emulsion_core::Command::TranslateNode {
            id: edge.source.shape,
            dx: 15.,
            dy: 20.,
        })
        .unwrap();
    let after = diagram::endpoint_position(&editor.doc, &edge.source, (0., 0.)).unwrap();
    assert_eq!(after, (before.0 + 15., before.1 + 20.));
    editor.undo();
    assert_eq!(&editor.doc, doc);
    let file = temp("loose-roundtrip.emu");
    crate::project::write(&imported.project, &file).unwrap();
    assert_eq!(crate::project::read(&file).unwrap().pages[0].doc, *doc);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn visio_floating_connector_move_keeps_owned_endpoints() {
    let xml = r#"<VisioDocument><Pages><Page ID="0"><Shapes><Shape ID="1" Type="1D"><Cell N="BeginX" V="1"/><Cell N="BeginY" V="1"/><Cell N="EndX" V="2"/><Cell N="EndY" V="2"/></Shape></Shapes></Page></Pages></VisioDocument>"#;
    let imported = visio::from_xml(xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    let model = doc.diagram.as_ref().unwrap();
    let (&id, edge) = model.edges.iter().next().unwrap();
    for endpoint in [&edge.source, &edge.target] {
        assert_eq!(doc.node(endpoint.shape).unwrap().parent, Some(id));
        assert!(model.shapes[&endpoint.shape].layout_locked);
    }
    let original = diagram::endpoint_position(doc, &edge.source, (0., 0.)).unwrap();
    let mut editor = emulsion_core::Editor::new(doc.clone(), None);
    editor
        .execute(emulsion_core::Command::TranslateNode {
            id,
            dx: 30.,
            dy: 20.,
        })
        .unwrap();
    assert_eq!(
        diagram::endpoint_position(&editor.doc, &edge.source, (0., 0.)).unwrap(),
        (original.0 + 30., original.1 + 20.)
    );
    editor.undo();
    assert_eq!(&editor.doc, doc);
    assert!(diagram::document_stencils(doc).is_empty());
}

#[test]
fn visio_ambiguous_contact_does_not_invent_attachment() {
    let mut xml = outlined_target().replace(
        "<Connect FromSheet=\"3\" FromCell=\"EndX\" ToSheet=\"2\"/>",
        "",
    );
    let start = xml.find("<Shape ID=\"2\"").unwrap();
    let end = start + xml[start..].find("</Shape>").unwrap() + 8;
    let duplicate = xml[start..end].replace("ID=\"2\"", "ID=\"4\"");
    xml.insert_str(end, &duplicate);
    let imported = visio::from_xml(&xml).unwrap();
    let doc = &imported.project.pages[0].doc;
    let model = doc.diagram.as_ref().unwrap();
    let edge = model.edges.values().next().unwrap();
    assert!(
        model.shapes[&edge.target.shape]
            .data
            .contains_key("emulsion_drawio_endpoint")
    );
}

fn outlined_target() -> String {
    VDX.replace("<Text>Ready?</Text>","<Text>Ready?</Text><Geom><MoveTo><X>0</X><Y>0</Y></MoveTo><LineTo><X>1</X><Y>0</Y></LineTo><LineTo><X>1</X><Y>0.5</Y></LineTo><LineTo><X>0</X><Y>0.5</Y></LineTo><LineTo><X>0</X><Y>0</Y></LineTo></Geom>")
}
