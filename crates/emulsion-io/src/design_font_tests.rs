//! Native project, template and brand interchange keep editable font resources.
use emulsion_core::{
    Command, Document, Editor, Node, NodeKind,
    command::Slot,
    design_fonts::{self, EmbeddedFont},
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
use std::io::Cursor;
#[test]
fn portable_font_project_template_brand_and_style_roundtrip() {
    let root = std::env::temp_dir().join(format!("emulsion-font-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let font = EmbeddedFont::from_bytes(include_bytes!("../../../assets/fonts/Geist.ttf").to_vec())
        .unwrap();
    let mut e = Editor::new(Document::new(320, 160), None);
    let id = e
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Headline",
                TextSpec {
                    text: "Portable headline".into(),
                    font: "Geist".into(),
                    size: 28.,
                    x: 10.,
                    y: 20.,
                    ..Default::default()
                },
                320,
                160,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let original = match &e.doc.node(id).unwrap().kind {
        NodeKind::Text { spec, .. } => emulsion_core::text::bounds(spec),
        _ => panic!(),
    };
    design_fonts::embed(&mut e, &[id], font.clone()).unwrap();
    let project = ProjectEditor::new_project(ProjectKind::Design, e.doc.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let mut bytes = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut bytes).unwrap();
    let restored = crate::project::read_from(Cursor::new(bytes.into_inner())).unwrap();
    let doc = &restored.pages[0].doc;
    assert_eq!(doc.design.fonts, e.doc.design.fonts);
    let NodeKind::Text { spec, .. } = &doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.font, font.alias());
    assert_eq!(emulsion_core::text::bounds(spec), original);
    let (svg, fallback) = crate::project_export::svg(doc).unwrap();
    assert!(!fallback);
    assert!(String::from_utf8(svg).unwrap().contains("<path"));
    let pack = root.join("portable.emutemplate");
    crate::template_pack::write(
        &restored,
        &crate::template_pack::Manifest::new(crate::template_pack::Kind::Design, "Portable".into()),
        &pack,
    )
    .unwrap();
    assert_eq!(
        crate::template_pack::read(&pack).unwrap().project.pages[0]
            .doc
            .design
            .fonts,
        doc.design.fonts
    );
    let mut catalog = crate::creative_library::Catalog::default();
    let brand = catalog
        .add_brand("Studio".into(), "Geist".into(), vec![[1, 2, 3, 0]])
        .unwrap();
    let kit = catalog.brands.iter_mut().find(|b| b.id == brand).unwrap();
    kit.font = font.alias().into();
    kit.fonts.insert(font.alias().into(), font.clone());
    kit.typography.insert(
        "Heading".into(),
        emulsion_core::design_brand_assets::TypographyRole {
            font: font.alias().into(),
            size: 42.,
            ..Default::default()
        },
    );
    kit.palettes
        .insert("Transparent".into(), vec![[20, 30, 40, 0]]);
    let path = root.join("studio.brand.json");
    crate::creative_library::export_brand(kit, &path).unwrap();
    let imported = crate::creative_library::import_brand(&root.join("catalog"), &path).unwrap();
    assert_eq!(imported.brands[0].fonts, kit.fonts);
    assert_eq!(imported.brands[0].typography, kit.typography);
    assert_eq!(imported.brands[0].palettes, kit.palettes);
    emulsion_core::design_styles::create(&mut e, id, "Headline").unwrap();
    let style = e.doc.design.saved_styles["Headline"].clone();
    let mut target = Editor::new(Document::new(320, 160), None);
    let to = target
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Target",
                TextSpec {
                    text: "Different words".into(),
                    ..Default::default()
                },
                320,
                160,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let before = target.doc.clone();
    emulsion_core::design_styles::apply_portable(
        &mut target,
        &[to],
        "Headline",
        &style,
        &e.doc.design.fonts,
    )
    .unwrap();
    assert_eq!(target.doc.design.fonts, e.doc.design.fonts);
    assert!(
        matches!(&target.doc.node(to).unwrap().kind,NodeKind::Text{spec,..}if spec.text=="Different words"&&spec.font==font.alias())
    );
    assert!(target.undo());
    assert_eq!(target.doc, before);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn portable_font_archives_deduplicate_history_blobs_and_validate_references() {
    use std::io::Read;
    let root = std::env::temp_dir().join(format!("emulsion-font-history-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("history.emu");
    let font = EmbeddedFont::from_bytes(include_bytes!("../../../assets/fonts/Geist.ttf").to_vec())
        .unwrap();
    let mut doc = Document::new(80, 80);
    doc.design.fonts.insert(font.alias().into(), font.clone());
    let id = Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Text",
            TextSpec {
                text: "A".into(),
                font: font.alias().into(),
                ..Default::default()
            },
            80,
            80,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    let mut graph = emulsion_core::graph::Graph::new(doc.clone(), "Start");
    for i in 1..=12 {
        doc.node_mut(id).unwrap().opacity = i as f32 / 13.;
        graph.record(&doc, "Opacity", false).unwrap();
    }
    crate::ora::write_full(&doc, Some(&graph), &path).unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(
        archive
            .file_names()
            .filter(|name| name.ends_with(".font"))
            .count(),
        2
    );
    let mut live = String::new();
    archive
        .by_name("emulsion.json")
        .unwrap()
        .read_to_string(&mut live)
        .unwrap();
    let live: serde_json::Value = serde_json::from_str(&live).unwrap();
    assert_eq!(live["fonts"], serde_json::json!([font.alias()]));
    // A resource-only Design becomes default after detaching blobs and may be omitted.
    assert!(live["design"]["fonts"].is_null() || live["design"]["fonts"] == serde_json::json!({}));
    let mut history = String::new();
    archive
        .by_name(crate::history::GRAPH)
        .unwrap()
        .read_to_string(&mut history)
        .unwrap();
    assert!(history.len() < font.bytes().len());
    let history: serde_json::Value = serde_json::from_str(&history).unwrap();
    for commit in history["commits"].as_array().unwrap() {
        assert_eq!(commit["doc"]["fonts"], serde_json::json!([font.alias()]));
    }
    let opened = crate::ora::read_full(&path).unwrap();
    assert_eq!(opened.doc.design.fonts, doc.design.fonts);
    let mut fonts = crate::font_data::FontPool::default();
    assert!(
        fonts
            .restore(
                &mut Default::default(),
                &["../../outside".into()],
                &mut archive,
                "fonts"
            )
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}
