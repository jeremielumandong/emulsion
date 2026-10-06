use emulsion_io::{open, open_full, open_full_with_report, psd};
use std::path::PathBuf;

#[path = "common/document_contents.rs"]
mod document_contents;
use document_contents::assert_document_contents;

#[test]
fn psd_open_returns_the_decision_from_its_reader_without_changing_the_document() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd/blending");
    for name in [
        "knockout-none-nested.psd",
        "knockout-deep-nested-pt.psd",
        "opacity-fill.psd",
    ] {
        let path = fixtures.join(name);
        let (expected, report) = psd::read_with_report(&path).unwrap();
        let (opened, actual_report) = open_full_with_report(&path).unwrap();
        assert_document_contents(&opened.doc, &expected, &format!("{name}: report bridge"));
        assert_eq!(actual_report, Some(report), "{name}");
        assert!(opened.graph.is_none());
        assert!(opened.history_error.is_none());
        assert_document_contents(
            &open_full(&path).unwrap().doc,
            &expected,
            &format!("{name}: legacy API"),
        );
        assert_document_contents(
            &open(&path).unwrap(),
            &expected,
            &format!("{name}: document API"),
        );
    }
}

#[test]
fn non_psd_open_has_no_profile_report_and_retains_source_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("source.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 30, 80, 128]))
        .save(&png)
        .unwrap();
    let expected = open_full(&png).unwrap().doc;
    let (opened, report) = open_full_with_report(&png).unwrap();
    assert!(report.is_none());
    assert_document_contents(&opened.doc, &expected, "PNG report bridge");
    let native = dir.path().join("source.ora");
    emulsion_io::save(&expected, &native).unwrap();
    let (opened, report) = open_full_with_report(&native).unwrap();
    assert!(report.is_none());
    assert_document_contents(
        &opened.doc,
        &open_full(&native).unwrap().doc,
        "ORA report bridge",
    );
    assert!(opened.history_error.is_none());
}

#[cfg(test)]
mod assertion_tests {
    use super::assert_document_contents;
    use emulsion_core::{Document, Node, NodeKind, SmartFilterMask, VectorMask};
    use emulsion_raster::{Mask, Placement, Raster};
    use std::sync::Arc;

    fn document() -> Document {
        let mut doc = Document::new(2, 2);
        let source = Arc::new(Raster::from_pixels(2, 2, [0; 4], &[[10, 20, 30, 40]; 4]));
        let mut raster = Node::raster(1, "Hidden source", source.clone(), Placement::default());
        raster.visible = false;
        raster.mask = Some(Arc::new(Mask::from_pixels(2, 2, 0, &[10, 20, 30, 40])));
        raster.vector_mask = Some(VectorMask {
            path: Arc::new(emulsion_raster::vector_geometry::rectangle(0., 0., 1., 1.)),
            ..Default::default()
        });
        let smart = Node::new(
            2,
            "Smart source",
            NodeKind::Smart {
                editable: Some(emulsion_core::node::SmartEditable::Svg {
                    xml: Arc::from("<svg/>"),
                }),
                original_image: Some(Arc::new(emulsion_core::node::OriginalImage::new(
                    Arc::new(vec![1, 2, 3]),
                    [4; 32],
                    [5; 32],
                ))),
                filter_mask: Some(SmartFilterMask::new(Arc::new(Mask::empty(2, 2, 128)))),
                source: source.clone(),
                filters: Vec::new(),
                filter_styles: Vec::new(),
                filters_enabled: true,
                placement: emulsion_core::SmartPlacement::Legacy(Placement::default()),
                cache: source,
                offset: (0, 0),
            },
        );
        doc.nodes = vec![raster, smart];
        doc.next_id = 3;
        doc.selection = Some(Arc::new(Mask::white(2, 2)));
        doc
    }

    #[test]
    fn content_assertion_accepts_independent_allocations_without_changing_equality() {
        let actual = document();
        let expected = document();
        assert_ne!(
            actual, expected,
            "production equality retains allocation identity"
        );
        assert_document_contents(&actual, &expected, "independent source allocations");
        assert_ne!(
            actual, expected,
            "the assertion must not mutate either input"
        );
    }

    #[test]
    fn content_assertion_rejects_metadata_pixels_masks_and_geometry_changes() {
        type Mutation = (&'static str, fn(&mut Document));
        let mutations: &[Mutation] = &[
            ("source depth", |d| d.source_depth = 16),
            ("node allocator", |d| d.next_id += 1),
            ("image metadata", |d| d.info = Some(Default::default())),
            ("project colors", |d| d.colors.push([1, 2, 3])),
            ("drawing guides", |d| {
                d.drawing_guides
                    .guides
                    .push(emulsion_core::drawing_guides::GuideKind::Grid { size: 10. })
            }),
            ("selection", |d| {
                d.selection = Some(Arc::new(Mask::empty(2, 2, 0)))
            }),
            ("hidden source pixel", |d| {
                let NodeKind::Raster { raster, .. } = &mut d.nodes[0].kind else {
                    unreachable!()
                };
                *raster = Arc::new(Raster::from_pixels(2, 2, [0; 4], &[[11, 20, 30, 40]; 4]));
            }),
            ("source dimensions", |d| {
                let NodeKind::Raster { raster, .. } = &mut d.nodes[0].kind else {
                    unreachable!()
                };
                *raster = Arc::new(Raster::from_pixels(
                    1,
                    4,
                    raster.fill(),
                    &raster.to_pixels(),
                ));
            }),
            ("source fill", |d| {
                let NodeKind::Raster { raster, .. } = &mut d.nodes[0].kind else {
                    unreachable!()
                };
                *raster = Arc::new(Raster::from_pixels(2, 2, [1; 4], &raster.to_pixels()));
            }),
            ("raster mask fill", |d| {
                let mask = d.nodes[0].mask.as_mut().unwrap();
                *mask = Arc::new(Mask::from_pixels(2, 2, 255, &mask.to_pixels()));
            }),
            ("vector geometry", |d| {
                let mask = d.nodes[0].vector_mask.as_mut().unwrap();
                Arc::make_mut(&mut mask.path).subpaths[0].anchors[0].p.0 = 0.25;
            }),
            ("mask transform", |d| {
                d.nodes[0].mask_transform = emulsion_core::Mapping2::Affine(
                    glam::DAffine2::from_translation(glam::dvec2(1., 0.)),
                )
            }),
            ("Smart source", |d| {
                let NodeKind::Smart { source, .. } = &mut d.nodes[1].kind else {
                    unreachable!()
                };
                *source = Arc::new(Raster::empty(2, 2, [0; 4]));
            }),
            ("original image", |d| {
                let NodeKind::Smart { original_image, .. } = &mut d.nodes[1].kind else {
                    unreachable!()
                };
                *original_image = Some(Arc::new(emulsion_core::node::OriginalImage::new(
                    Arc::new(vec![9]),
                    [4; 32],
                    [5; 32],
                )));
            }),
            ("Smart Filter mask pixels", |d| {
                let NodeKind::Smart { filter_mask, .. } = &mut d.nodes[1].kind else {
                    unreachable!()
                };
                filter_mask.as_mut().unwrap().pixels = Arc::new(Mask::empty(2, 2, 0));
            }),
            ("Smart Filter mask controls", |d| {
                let NodeKind::Smart { filter_mask, .. } = &mut d.nodes[1].kind else {
                    unreachable!()
                };
                filter_mask.as_mut().unwrap().enabled = false;
            }),
            ("Smart cache pixels", |d| {
                let NodeKind::Smart { cache, .. } = &mut d.nodes[1].kind else {
                    unreachable!()
                };
                *cache = Arc::new(Raster::empty(2, 2, [0; 4]));
            }),
            ("Smart cache offset", |d| {
                let NodeKind::Smart { offset, .. } = &mut d.nodes[1].kind else {
                    unreachable!()
                };
                offset.0 = 1;
            }),
        ];
        let expected = document();
        for (name, mutate) in mutations {
            let mut changed = document();
            mutate(&mut changed);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_document_contents(&changed, &expected, name);
            }));
            assert!(result.is_err(), "content assertion missed {name}");
        }
    }
}
