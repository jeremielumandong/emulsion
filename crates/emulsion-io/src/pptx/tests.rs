use super::*;
use emulsion_core::{
    Command, Editor,
    command::Slot,
    design_interactions::Action,
    project::ProjectEditor,
    text::{TextRun, TextSpec},
};
use std::io::{Read, Write};
fn add(e: &mut Editor, node: Node, parent: Option<u64>) -> u64 {
    e.execute(Command::AddNode {
        node: Box::new(node),
        slot: Slot::top_of(parent),
    })
    .unwrap()
    .unwrap()
}
pub(super) fn fixture() -> Project {
    let mut e = Editor::new(Document::new(800, 450), None);
    let group = add(&mut e, Node::group(0, "Editable group"), None);
    add(
        &mut e,
        Node::path(
            0,
            "Bezier shape",
            Arc::new(VectorPath::from_svg("M40 40 C70 10 130 10 160 40 L160 90 L40 90 Z").unwrap()),
            PathStyle {
                fill: Some([220, 70, 90, 200]),
                stroke: Some([40, 40, 40, 255]),
                width: 3.,
                ..Default::default()
            },
            800,
            450,
        ),
        Some(group),
    );
    let mut spec = TextSpec {
        text: "Hello 日本語\nEditable slides".into(),
        font: "DejaVu Sans".into(),
        size: 28.,
        x: 60.,
        y: 140.,
        width: Some(480.),
        height: Some(150.),
        rotation: 12.,
        ..Default::default()
    };
    let mut bold = spec.base_style();
    bold.bold = true;
    bold.underline = true;
    bold.color = [20, 70, 210, 255];
    spec.runs.push(TextRun {
        start: 0,
        end: 5,
        style: bold,
    });
    let text = add(
        &mut e,
        Node::text(0, "Rich text", spec, 800, 450),
        Some(group),
    );
    let image = add(
        &mut e,
        Node::raster(
            0,
            "Photo",
            Arc::new(Raster::from_srgba8(
                2,
                2,
                &[
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 128,
                ],
            )),
            Placement {
                x: 550.,
                y: 80.,
                scale_x: 40.,
                scale_y: 30.,
                rotation: 15.,
                ..Default::default()
            },
        ),
        None,
    );
    e.doc.design.speaker_notes = "Speaker notes & details\nSecond line".into();
    e.doc.design.interactions.insert(
        text,
        vec![Action::Url {
            url: "https://example.com/slides?a=1&b=2".into(),
        }],
    );
    e.doc
        .design
        .interactions
        .insert(image, vec![Action::Slide { page: 2 }]);
    let mut project = ProjectEditor::new_project(ProjectKind::Design, e.doc).unwrap();
    project
        .add_page(Document::new(800, 450), "Second slide".into(), 0.)
        .unwrap();
    project.snapshot().unwrap()
}
#[test]
fn pptx_roundtrip_preserves_editable_objects_runs_notes_order_links_and_history() {
    let project = fixture();
    let before = project.pages[0].doc.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("editable.pptx");
    let report = write(&project, &[2, 1], &path).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let imported = read(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.pages[0].meta.name, "Second slide");
    let doc = &imported.project.pages[1].doc;
    assert_eq!(
        doc.design.interactions.len(),
        2,
        "child links must not become group links"
    );
    assert_eq!(doc.design.speaker_notes, before.design.speaker_notes);
    let text = doc.nodes.iter().find(|n| n.name == "Rich text").unwrap();
    let NodeKind::Text { spec, .. } = &text.kind else {
        panic!("text was rasterized")
    };
    assert_eq!(spec.text, "Hello 日本語\nEditable slides");
    assert!(spec.style_at(0).bold && spec.style_at(0).underline);
    assert!(!spec.style_at(6).bold);
    assert!((spec.rotation - 12.).abs() < 0.01);
    assert!(text.parent.is_some());
    assert!(
        doc.nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Path { .. }))
    );
    assert!(
        doc.nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Raster { .. }))
    );
    assert!(
        matches!(&doc.design.interactions[&text.id][0],Action::Url{url} if url.contains("a=1&b=2"))
    );
    let photo = doc.nodes.iter().find(|n| n.name == "Photo").unwrap();
    assert_picture_matches(
        photo,
        before.nodes.iter().find(|n| n.name == "Photo").unwrap(),
    );
    assert_eq!(
        doc.design.interactions[&photo.id],
        vec![Action::Slide { page: 1 }]
    );
    let mut session =
        ProjectEditor::new_project(ProjectKind::Design, Document::new(100, 100)).unwrap();
    let original = session.stamp();
    let ids = session.import_pages(imported.project).unwrap();
    assert_eq!(ids.len(), 2);
    session.undo();
    assert_eq!(session.stamp(), original);
    assert_eq!(project.pages[0].doc, before);
    let native = dir.path().join("imported.emu");
    crate::project::write(&read(&path).unwrap().project, &native).unwrap();
    let restored = crate::project::read(&native).unwrap();
    let mut expected = read(&path).unwrap().project.pages.remove(1).doc;
    // Document equality deliberately compares raster buffers by Arc identity.
    // Verify the decoded pixels before sharing that identity for the metadata check.
    for node in &mut expected.nodes {
        if let NodeKind::Raster { raster, .. } = &mut node.kind {
            let actual = restored.pages[1].doc.node(node.id).unwrap();
            let NodeKind::Raster { raster: saved, .. } = &actual.kind else {
                panic!("native save changed an editable picture's kind")
            };
            assert_eq!(
                (raster.width(), raster.height()),
                (saved.width(), saved.height())
            );
            assert_eq!(raster.to_srgba8(), saved.to_srgba8());
            *raster = saved.clone();
        }
    }
    assert_eq!(restored.pages[1].doc, expected);
}
fn assert_picture_matches(actual: &Node, expected: &Node) {
    let NodeKind::Raster { raster, placement } = &actual.kind else {
        panic!("picture is no longer an editable raster")
    };
    let NodeKind::Raster {
        raster: expected_raster,
        placement: expected_placement,
    } = &expected.kind
    else {
        panic!("expected picture fixture")
    };
    assert_eq!(raster.to_srgba8(), expected_raster.to_srgba8());
    for (actual, expected) in placement
        .to_doc(raster.width(), raster.height())
        .to_cols_array()
        .into_iter()
        .zip(
            expected_placement
                .to_doc(expected_raster.width(), expected_raster.height())
                .to_cols_array(),
        )
    {
        assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
    }
}

#[test]
fn nested_rotated_pictures_and_later_siblings_survive_a_failed_picture() {
    let project = fixture();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested-pictures.pptx");
    write(&project, &[1, 2], &path).unwrap();
    rewrite(&path, "ppt/slides/slide1.xml", |xml| {
        let start = xml.find("<p:pic>").unwrap();
        let end = start + xml[start..].find("</p:pic>").unwrap() + "</p:pic>".len();
        let picture = &xml[start..end];
        let parsed = package::parse(picture).unwrap();
        let id = parsed.descendants("cNvPr").next().unwrap().attr("id");
        let width = parsed
            .child("spPr")
            .unwrap()
            .child("xfrm")
            .unwrap()
            .child("ext")
            .unwrap()
            .attr("cx");
        let nested = picture
            .replace("name=\"Photo\"", "name=\"Nested photo\"")
            .replace(&format!("id=\"{id}\""), "id=\"901\"");
        let failed = picture
            .replace("name=\"Photo\"", "name=\"Invalid photo\"")
            .replace(&format!("id=\"{id}\""), "id=\"902\"")
            .replace(&format!("cx=\"{width}\""), "cx=\"0\"");
        let group = format!(
            "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"900\" name=\"Picture group\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{failed}{nested}</p:grpSp>"
        );
        xml.replacen("</p:grpSp>", &format!("{group}</p:grpSp>"), 1)
    });
    let imported = read(&path).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("Invalid photo: Object could not be imported:"));
    let page = &imported.project.pages[0];
    let doc = &page.doc;
    doc.validate().unwrap();
    assert_eq!(
        doc.nodes
            .iter()
            .map(|n| n.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Slide background",
            "Bezier shape",
            "Rich text",
            "Nested photo",
            "Picture group",
            "Editable group",
            "Photo"
        ]
    );
    // A failed picture consumes neither a node ID nor an interaction; objects
    // already imported and the later root picture retain their order and links.
    assert_eq!(doc.next_id, 8);
    assert_eq!(
        doc.nodes.iter().map(|n| n.id).collect::<HashSet<_>>(),
        (1..8).collect()
    );
    let outer = doc
        .nodes
        .iter()
        .find(|n| n.name == "Editable group")
        .unwrap();
    let group = doc
        .nodes
        .iter()
        .find(|n| n.name == "Picture group")
        .unwrap();
    let nested = doc.nodes.iter().find(|n| n.name == "Nested photo").unwrap();
    let photo = doc.nodes.iter().find(|n| n.name == "Photo").unwrap();
    let expected = project.pages[0]
        .doc
        .nodes
        .iter()
        .find(|n| n.name == "Photo")
        .unwrap();
    assert_eq!(group.parent, Some(outer.id));
    assert_eq!(nested.parent, Some(group.id));
    assert_eq!(photo.parent, None);
    assert_picture_matches(nested, expected);
    assert_picture_matches(photo, expected);
    assert_eq!(doc.design.interactions.len(), 3);
    for picture in [nested, photo] {
        assert_eq!(
            doc.design.interactions[&picture.id],
            vec![Action::Slide { page: 2 }]
        );
    }
    assert!(!doc.design.interactions.contains_key(&outer.id));
    assert!(!doc.design.interactions.contains_key(&group.id));
    assert_eq!(page.graph.commits().count(), 1);
    assert_eq!(&page.graph.commits().next().unwrap().doc, doc);
}

#[test]
fn identical_run_links_remain_one_object_action_without_group_inheritance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.pptx");
    write(&fixture(), &[1, 2], &path).unwrap();
    rewrite(&path, "ppt/slides/slide1.xml", |xml| {
        let parsed = package::parse(&xml).unwrap();
        let link = parsed
            .descendants("hlinkClick")
            .find(|l| l.attr("action").is_empty())
            .unwrap();
        xml.replace(
            "</a:rPr>",
            &format!("<a:hlinkClick r:id=\"{}\"/></a:rPr>", link.attr("r:id")),
        )
    });
    let imported = read(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let doc = &imported.project.pages[0].doc;
    assert_eq!(doc.design.interactions.len(), 2);
    let group = doc
        .nodes
        .iter()
        .find(|n| n.name == "Editable group")
        .unwrap();
    assert!(!doc.design.interactions.contains_key(&group.id));
}
fn rewrite(path: &Path, part: &str, modify: impl FnOnce(String) -> String) {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut files = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        files.push((f.name().to_string(), data));
    }
    drop(zip);
    let item = files.iter_mut().find(|(n, _)| n == part).unwrap();
    item.1 = modify(String::from_utf8(item.1.clone()).unwrap()).into_bytes();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, data) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();
}
#[test]
fn pptx_limits_relationships_diagnostics_and_atomic_export_are_enforced() {
    let p = fixture();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.pptx");
    let mut appearance = p.clone();
    let page = &mut appearance.pages[0];
    page.doc
        .nodes
        .iter_mut()
        .find(|n| n.name == "Editable group")
        .unwrap()
        .opacity = 0.5;
    page.doc
        .nodes
        .iter_mut()
        .find(|n| n.name == "Bezier shape")
        .unwrap()
        .blending
        .fill_opacity = 0.5;
    page.graph = Graph::new(page.doc.clone(), "Appearance compatibility fixture");
    let report = write(&appearance, &[1, 2], &path).unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Editable group") && w.contains("Group opacity"))
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Bezier shape") && w.contains("blend modes"))
    );
    write(&p, &[1], &path).unwrap();
    rewrite(&path, "ppt/slides/slide1.xml", |s| {
        s.replace("</p:spTree>","<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"99\" name=\"Chart 99\"/></p:nvGraphicFramePr><p:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"952500\" cy=\"476250\"/></p:xfrm><a:graphic/></p:graphicFrame></p:spTree>")
    });
    let imported = read(&path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("Chart 99") && w.contains("placeholder"))
    );
    assert!(
        imported.project.pages[0]
            .doc
            .nodes
            .iter()
            .any(|n| n.name == "Rich text")
    );
    let previous = std::fs::read(&path).unwrap();
    assert!(write(&p, &[99], &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), previous);
    rewrite(&path, "ppt/_rels/presentation.xml.rels", |s| {
        s.replace("slides/slide1.xml", "../../../escape.xml")
    });
    assert!(read(&path).is_err());
    assert!(
        package::parse("<!DOCTYPE x [<!ENTITY y SYSTEM 'file:///etc/passwd'>]><x>&y;</x>").is_err()
    );
    assert!(package::parse(&format!("{}{}", "<x>".repeat(66), "</x>".repeat(66))).is_err());
}
#[test]
fn pptx_external_links_require_click_and_safe_schemes() {
    let mut e = Editor::new(Document::new(100, 100), None);
    let id = add(&mut e, Node::group(0, "Link"), None);
    let before = e.doc.clone();
    for url in [
        "javascript:alert(1)",
        "file:///tmp/a",
        "https://user:pass@example.com",
        "https://",
        "https://example.com/\n",
    ] {
        assert!(
            emulsion_core::design_interactions::author(
                &mut e,
                id,
                vec![Action::Url { url: url.into() }],
                None
            )
            .is_err()
        );
        assert_eq!(e.doc, before);
    }
    assert!(
        emulsion_core::design_interactions::author_with_trigger(
            &mut e,
            id,
            vec![Action::Url {
                url: "https://example.com".into()
            }],
            None,
            Some(emulsion_core::design_interactions::Trigger::Hover)
        )
        .is_err()
    );
    assert_eq!(e.doc, before);
}
