//! Appearance-only interchange must not silently discard native vector masks.
use emulsion_core::{
    Command, Document, EmptyVectorCoverage, Node, VectorMask,
    command::Slot,
    project::{ProjectEditor, ProjectKind},
};
use emulsion_io::{lottie, pptx, template_pack};
use emulsion_raster::vector::{Path, PathStyle};
use std::sync::Arc;

fn masked_shape(enabled: bool) -> Document {
    let mut doc = Document::new(32, 24);
    let mut node = Node::path(
        0,
        "Vector masked shape",
        Arc::new(Path::from_svg("M 2 2 L 30 2 L 30 22 L 2 22 Z").unwrap()),
        PathStyle::default(),
        doc.width,
        doc.height,
    );
    node.vector_mask = Some(VectorMask {
        enabled,
        ..VectorMask::empty(EmptyVectorCoverage::HideAll)
    });
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    doc
}

#[test]
fn lottie_rejects_enabled_or_disabled_native_vector_masks_explicitly() {
    for enabled in [false, true] {
        let doc = masked_shape(enabled);
        let error = lottie::encode(&doc).unwrap_err().to_string();
        assert!(error.contains("masks"), "{error}");
        assert!(error.contains("rendered-frame export"), "{error}");
        let mut plain = doc.clone();
        plain.nodes[0].vector_mask = None;
        lottie::encode(&plain).unwrap();
    }
}

#[test]
fn pptx_refuses_vector_masks_without_exposing_unmasked_base_artwork() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mask.pptx");
    for enabled in [false, true] {
        for visible in [false, true] {
            let mut doc = masked_shape(enabled);
            doc.nodes[0].visible = visible;
            let project = ProjectEditor::new_project(ProjectKind::Design, doc)
                .unwrap()
                .snapshot()
                .unwrap();
            let before = project.clone();
            std::fs::write(&path, b"existing export must survive").unwrap();
            let error = pptx::write(&project, &[project.pages[0].meta.id], &path)
                .unwrap_err()
                .to_string();
            assert!(error.contains("native vector mask"), "{error}");
            assert!(error.contains("rendered appearance export"), "{error}");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                b"existing export must survive"
            );
            assert_eq!(project.pages[0].doc, before.pages[0].doc);
        }
    }
}

#[test]
fn stencil_export_preserves_vector_masked_fill_artwork_even_when_disabled() {
    for enabled in [false, true] {
        let mut doc = Document::new(32, 24);
        let mut fill = Node::new(
            0,
            "Masked fill artwork",
            emulsion_core::NodeKind::Fill {
                rgba: [120, 50, 180, 255],
            },
        );
        fill.vector_mask = Some(VectorMask {
            enabled,
            ..VectorMask::default()
        });
        Command::AddNode {
            node: Box::new(fill),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let project = ProjectEditor::new_project(ProjectKind::Diagram, doc)
            .unwrap()
            .snapshot()
            .unwrap();
        let prepared = template_pack::stencil_project(&project).unwrap();
        let retained = prepared.pages[0]
            .doc
            .nodes
            .iter()
            .find(|n| n.name == "Masked fill artwork")
            .unwrap();
        assert_eq!(retained.vector_mask.as_ref().unwrap().enabled, enabled);
        assert_eq!(
            project.pages[0]
                .doc
                .nodes
                .iter()
                .find(|n| n.name == "Masked fill artwork")
                .unwrap()
                .vector_mask,
            retained.vector_mask
        );
    }
}
