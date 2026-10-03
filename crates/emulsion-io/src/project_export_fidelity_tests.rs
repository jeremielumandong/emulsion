//! Print/export fidelity contract: native source stays editable; output size,
//! resolution and fallback reporting describe the actual exported appearance.
use super::*;
use emulsion_core::{
    Command, Node,
    command::Slot,
    project::{ProjectEditor, ProjectKind},
    styles::LayerStyle,
};
use std::{io::Read, sync::Arc};

fn fixture() -> Project {
    let mut doc = Document::new(150, 100);
    doc.resolution = 300.;
    Command::AddNode {
        node: Box::new(Node::new(
            0,
            "Background",
            NodeKind::Fill {
                rgba: [15, 20, 30, 255],
            },
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    let mut node = Node::path(
        0,
        "Native glowing shape",
        Arc::new(emulsion_raster::vector_geometry::rectangle(
            40., 30., 60., 30.,
        )),
        emulsion_raster::vector::PathStyle {
            fill: Some([240, 150, 60, 255]),
            stroke: None,
            ..Default::default()
        },
        150,
        100,
    );
    node.styles.push(LayerStyle::DropShadow {
        color: [0, 180, 255],
        opacity: 90.,
        angle: 90.,
        distance: 0.,
        size: 14.,
    });
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    ProjectEditor::new_project(ProjectKind::Design, doc)
        .unwrap()
        .snapshot()
        .unwrap()
}

#[test]
fn page_png_retains_exact_canvas_samples_print_resolution_and_source() {
    let project = fixture();
    let original = project.clone();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pixels.zip");
    let report = write(&project, &[1], Format::Png, false, &path).unwrap();
    assert_eq!(report.pages, 1);
    assert!(report.rasterized_effect_pages.is_empty());
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_index(0).unwrap().read_to_end(&mut bytes).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().into_rgba16();
    let canvas = flatten(&project.pages[0].doc.composite_tree(), 0);
    assert_eq!(image.dimensions(), (150, 100));
    assert_eq!(image.into_raw(), canvas.to_srgba16());
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = decoder.read_info().unwrap();
    let density = reader.info().pixel_dims.unwrap();
    assert_eq!(density.unit, png::Unit::Meter);
    assert_eq!((density.xppu, density.yppu), (11811, 11811));
    assert!(reader.info().icc_profile.is_some());
    assert_eq!(project.pages[0].doc, original.pages[0].doc);
}

#[test]
fn pdf_reports_native_effect_fallback_and_keeps_print_size_and_native_roundtrip() {
    let project = fixture();
    let original = project.clone();
    let directory = tempfile::tempdir().unwrap();
    let pdf = directory.path().join("effects.pdf");
    let report = write(&project, &[1], Format::Pdf, false, &pdf).unwrap();
    assert_eq!(
        report.rasterized_pages,
        vec![project.pages[0].meta.name.clone()]
    );
    assert!(report.rasterized_effect_pages.is_empty());
    let bytes = std::fs::read(pdf).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Image"));
    assert!(text.contains("/MediaBox [0 0 36 24]"), "{text}");
    assert!(text.contains("/TrimBox [0 0 36 24]"));
    let native = directory.path().join("effects.emu");
    crate::project::write(&project, &native).unwrap();
    let reopened = crate::project::read(&native).unwrap();
    assert_eq!(reopened.pages[0].doc, original.pages[0].doc);
    assert_eq!(project.pages[0].doc, original.pages[0].doc);
    assert!(
        project.pages[0]
            .doc
            .nodes
            .iter()
            .any(|node| matches!(node.kind, NodeKind::Path { .. }) && !node.styles.is_empty())
    );
}

#[test]
fn native_background_coverage_warning_is_exported_with_selected_page_names() {
    let mut editor = emulsion_core::Editor::new(Document::new(40, 30), None);
    editor.doc.resolution = 100.;
    emulsion_core::design_background::replace_image(
        &mut editor,
        Arc::new(emulsion_raster::Raster::solid(40, 30, [0.2, 0.4, 0.8, 1.])),
    )
    .unwrap();
    let mut project = ProjectEditor::new_project(ProjectKind::Design, editor.doc)
        .unwrap()
        .snapshot()
        .unwrap();
    project.pages[0].meta.bleed_mm = 2.54;
    let original = project.pages[0].doc.clone();
    let directory = tempfile::tempdir().unwrap();
    for (include_bleed, expected) in [(false, 0), (true, 1)] {
        let path = directory.path().join("background.zip");
        let report = write(&project, &[1], Format::Png, include_bleed, &path).unwrap();
        assert_eq!(report.insufficient_bleed_pages.len(), expected);
        if include_bleed {
            assert_eq!(
                report.insufficient_bleed_pages[0],
                project.pages[0].meta.name
            );
        }
        assert_eq!(project.pages[0].doc, original);
    }
}

#[test]
fn pdf_without_native_effects_stays_vector() {
    let mut project = fixture();
    for node in &mut project.pages[0].doc.nodes {
        node.effects_enabled = false;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vectors.pdf");
    let report = write(&project, &[1], Format::Pdf, false, &path).unwrap();
    assert!(report.rasterized_pages.is_empty());
    assert!(report.rasterized_page_ppi.is_empty());
    assert!(report.rasterized_effect_pages.is_empty());
    let bytes = std::fs::read(path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("/Subtype /Image"));
}

#[test]
fn invisible_native_shadows_do_not_flatten_other_vector_artwork() {
    for opacity in [0., 1.] {
        let mut project = fixture();
        let doc = &mut project.pages[0].doc;
        let styled = doc
            .nodes
            .iter_mut()
            .find(|node| !node.styles.is_empty())
            .unwrap();
        styled.opacity = opacity;
        styled.visible = opacity == 0.;
        let (svg, fallback) = pdf_svg(doc).unwrap();
        assert!(!fallback);
        assert!(!String::from_utf8_lossy(&svg).contains("<image"));
    }
}

#[test]
fn zero_opacity_ancestor_does_not_flatten_visible_vector_page() {
    let mut project = fixture();
    let doc = &mut project.pages[0].doc;
    let index = doc
        .nodes
        .iter()
        .position(|node| !node.styles.is_empty())
        .unwrap();
    let id = doc.next_id;
    doc.next_id += 1;
    doc.nodes[index].parent = Some(id);
    let mut group = Node::group(id, "Hidden effect group");
    group.opacity = 0.;
    doc.nodes.insert(index, group);
    doc.normalize();
    doc.validate().unwrap();
    let (svg, fallback) = pdf_svg(doc).unwrap();
    assert!(!fallback);
    assert!(!String::from_utf8_lossy(&svg).contains("<image"));
}
