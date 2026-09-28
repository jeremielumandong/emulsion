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
    assert_eq!(imported.project.pages[0].doc.diagram.as_ref().unwrap().shapes.len(), 1);
    assert!(imported.warnings.iter().any(|w| w.contains("instead of masters")));
    std::fs::remove_file(file).unwrap();
}
