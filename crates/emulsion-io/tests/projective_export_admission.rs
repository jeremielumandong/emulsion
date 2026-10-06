//! Export capability gates depend on retained variants, including dormant maps.
use emulsion_core::{
    Document, Editor, Mapping2, Node, NodeKind, SmartFilterMask, SmartPlacement,
    node::SmartEditable,
    project::{ProjectEditor, ProjectKind},
};
use emulsion_io::{IoError, project_export, psd, smart_source};
use emulsion_raster::{Mask, Placement, Raster, projective::Projective2};
use std::sync::Arc;

fn fixture(feature: usize, visible: bool) -> Document {
    let mut doc = Document::new(8, 8);
    let mut node = Node::smart(
        1,
        "Retained projective state",
        Arc::new(Raster::solid(4, 4, [1., 0., 0., 1.])),
        vec![],
        Placement::default(),
    );
    node.visible = visible;
    match feature {
        0 => {
            let NodeKind::Smart { placement, .. } = &mut node.kind else {
                unreachable!()
            };
            *placement = SmartPlacement::Projective(Projective2::IDENTITY);
        }
        1 => {
            node.mask = Some(Arc::new(Mask::empty(4, 4, 255)));
            node.mask_enabled = false;
            node.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
        }
        2 => {
            // No plane exists, but native metadata still must not be dropped.
            node.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
        }
        3 => {
            let NodeKind::Smart {
                filter_mask,
                filters_enabled,
                ..
            } = &mut node.kind
            else {
                unreachable!()
            };
            let mut mask = SmartFilterMask::new(Arc::new(Mask::empty(4, 4, 255)));
            mask.enabled = false;
            mask.transform = Mapping2::Projective(Projective2::IDENTITY);
            *filter_mask = Some(mask);
            *filters_enabled = false;
        }
        _ => unreachable!(),
    }
    doc.nodes.push(node);
    doc.next_id = 2;
    doc.validate().unwrap();
    doc
}

#[test]
fn projective_variants_select_reported_svg_and_psd_appearance_fallback() {
    let scratch = tempfile::tempdir().unwrap();
    for feature in 0..4 {
        for visible in [false, true] {
            let doc = fixture(feature, visible);
            let before = doc.clone();
            assert!(project_export::vector_svg(&doc).is_err());
            let (svg, fallback) = project_export::svg(&doc).unwrap();
            assert!(fallback);
            assert!(String::from_utf8(svg).unwrap().contains("data:image/png"));
            assert!(psd::needs_appearance_fallback(&doc));
            let path = scratch.path().join(format!("{feature}-{visible}.psd"));
            let report = psd::write_with_report(&doc, &path).unwrap();
            assert_eq!(
                report.appearance_fallback,
                Some(psd::AppearanceFallback::UnsupportedFeatures)
            );
            assert!(!report.baked_raster_masks);
            assert_eq!(doc, before);
            assert!(doc.nodes[0].has_projective_metadata());
        }
    }
}

#[test]
fn editable_svg_source_with_latent_projective_mask_is_not_embedded() {
    let mut doc = fixture(2, true);
    let NodeKind::Smart { editable, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    *editable = Some(SmartEditable::Svg {
        xml: Arc::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\"><rect width=\"4\" height=\"4\" fill=\"red\"/></svg>",
        ),
    });
    let (svg, fallback) = project_export::svg(&doc).unwrap();
    assert!(fallback);
    assert!(
        !String::from_utf8(svg)
            .unwrap()
            .contains("data:image/svg+xml")
    );
    assert!(project_export::vector_svg(&doc).is_err());
}

#[test]
fn pptx_refuses_projective_metadata_before_replacing_the_destination() {
    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join("keep.pptx");
    for feature in 0..4 {
        std::fs::write(&path, b"keep existing destination").unwrap();
        let project = ProjectEditor::new_project(ProjectKind::Design, fixture(feature, false))
            .unwrap()
            .snapshot()
            .unwrap();
        let error =
            emulsion_io::pptx::write(&project, &[project.pages[0].meta.id], &path).unwrap_err();
        assert!(matches!(error, IoError::Geometry(_)));
        assert_eq!(std::fs::read(&path).unwrap(), b"keep existing destination");
    }
}

#[test]
fn source_replacement_refuses_before_external_reads_or_creating_a_file() {
    let scratch = tempfile::tempdir().unwrap();
    for feature in 0..4 {
        let mut editor = Editor::try_new(fixture(feature, true), None).unwrap();
        let before = editor.doc.clone();
        let revision = editor.revision;
        let path = scratch.path().join(format!("source-{feature}.ora"));
        assert!(matches!(
            smart_source::apply_changed(&mut editor, 1, &Document::new(4, 4)),
            Err(IoError::Geometry(_))
        ));
        assert!(matches!(
            smart_source::relink(&mut editor, 1, &path, false),
            Err(IoError::Geometry(_))
        ));
        assert!(matches!(
            smart_source::refresh(&mut editor, 1, false),
            Err(IoError::Geometry(_))
        ));
        assert!(matches!(
            smart_source::save_as(&mut editor, 1, &path),
            Err(IoError::Geometry(_))
        ));
        assert!(!path.exists());
        assert_eq!(editor.doc, before);
        assert_eq!(editor.revision, revision);
    }
}

#[test]
fn raw_export_refuses_projective_replacement_before_loading_original_or_replacing_output() {
    use emulsion_core::raw::{DevelopParams, RawDocument, RawMetadata};
    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join("keep.png");
    for feature in 0..4 {
        let mut doc = fixture(feature, true);
        doc.raw = Some(RawDocument {
            schema_version: 1,
            node_id: 1,
            source: scratch.path().join("missing.dng"),
            source_sha256: "0".repeat(64),
            params: DevelopParams::default(),
            metadata: RawMetadata::default(),
        });
        std::fs::write(&path, b"keep existing output").unwrap();
        assert!(matches!(
            emulsion_io::export::export(
                &doc,
                &path,
                emulsion_io::export::ExportOptions::for_doc(&doc)
            ),
            Err(IoError::Geometry(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"keep existing output");
    }
}
