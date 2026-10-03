use super::*;
use emulsion_core::{Command, Editor, Node, NodeKind, command::Slot, design_background};
use emulsion_raster::{Raster, composite::flatten, vector::PathStyle, vector_geometry};
use std::sync::Arc;

fn fixture(zoom: f64) -> Document {
    let mut editor = Editor::new(Document::new(40, 30), None);
    editor.doc.resolution = 100.;
    design_background::set_color(&mut editor, [0, 255, 0, 255]).unwrap();
    let photo = Arc::new(Raster::from_fn(40, 30, [0; 4], |x, y| {
        [
            10_000 + x as u16 * 300,
            1_000 + y as u16 * 100,
            40_000,
            u16::MAX,
        ]
    }));
    let image = design_background::replace_image(&mut editor, photo).unwrap();
    let crop = emulsion_core::design::crop_frame_image(&editor.doc, image, [0.; 2], zoom).unwrap();
    editor.execute(crop).unwrap();
    editor
        .execute(Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Foreground marker",
                Arc::new(vector_geometry::rectangle(10., 10., 4., 4.)),
                PathStyle {
                    fill: Some([255, 0, 0, 255]),
                    stroke: None,
                    ..Default::default()
                },
                40,
                30,
            )),
            slot: Slot::TOP,
        })
        .unwrap();
    editor.doc.validate().unwrap();
    editor.doc
}

#[test]
fn bleed_reveals_off_page_native_background_photo() {
    let source = fixture(2.);
    let original = source.clone();
    let background = design_background::parts(&source).unwrap().image.unwrap();
    let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
    assert_eq!(bleed, 10);
    assert_eq!((expanded.width, expanded.height), (60, 50));

    // The unclipped native photo is a reference for real off-page source pixels.
    let mut reference = expanded.clone();
    reference.node_mut(background.image).unwrap().clip_to = None;
    let expected = flatten(&reference.composite_tree(), 0);
    let actual = flatten(&expanded.composite_tree(), 0);
    assert_eq!(actual.get(3, 25), expected.get(3, 25));
    assert_eq!(actual.get(30, 3), expected.get(30, 3));
    assert_eq!(actual.get(57, 25), expected.get(57, 25));
    assert_eq!(actual.get(30, 47), expected.get(30, 47));
    let trim = flatten(&source.composite_tree(), 0);
    for y in 0..source.height {
        for x in 0..source.width {
            assert_eq!(actual.get(x + bleed, y + bleed), trim.get(x, y));
        }
    }
    assert_eq!(source, original);
}

fn assert_trim_unchanged(source: &Document, expanded: &Document, bleed: u32) {
    let before = flatten(&source.composite_tree(), 0);
    let after = flatten(&expanded.composite_tree(), 0);
    for y in 0..source.height {
        for x in 0..source.width {
            assert_eq!(before.get(x, y), after.get(x + bleed, y + bleed));
        }
    }
}

#[test]
fn zero_and_subpixel_bleed_leave_native_geometry_unchanged() {
    let source = fixture(1.);
    for mm in [0., 0.01] {
        let (expanded, bleed) = with_bleed(&source, mm).unwrap();
        assert_eq!(bleed, 0);
        assert_eq!(expanded, source);
        assert_eq!(background_photo_warning(&expanded, bleed), None);
    }
}

#[test]
fn bleed_uses_page_resolution_and_rounds_once() {
    for (resolution, expected) in [(72., 7), (100., 10), (300., 30), (600., 60)] {
        let mut source = fixture(2.);
        source.resolution = resolution;
        let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
        assert_eq!(bleed, expected);
        assert_eq!(expanded.width, source.width + 2 * expected);
        assert_eq!(expanded.height, source.height + 2 * expected);
        assert_eq!(expanded.resolution, resolution);
    }
}

#[test]
fn invalid_bleed_resolution_and_canvas_growth_return_errors_without_overflow() {
    let source = fixture(1.);
    for bleed in [
        -1.,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        1e12,
    ] {
        assert!(with_bleed(&source, bleed).is_err(), "{bleed}");
    }
    for resolution in [0., -100., f32::NAN, f32::INFINITY] {
        let mut source = source.clone();
        source.resolution = resolution;
        assert!(with_bleed(&source, 2.54).is_err(), "{resolution}");
    }
    let mut side_limit = Document::new(emulsion_core::document::MAX_SIDE - 2, 1);
    side_limit.resolution = 254.;
    let (largest, bleed) = with_bleed(&side_limit, 0.1).unwrap();
    assert_eq!(bleed, 1);
    assert_eq!(largest.width, emulsion_core::document::MAX_SIDE);
    assert!(with_bleed(&side_limit, 0.2).is_err());
    let mut pixel_limit = Document::new(20_000, 20_000);
    pixel_limit.resolution = 254.;
    assert!(with_bleed(&pixel_limit, 0.1).is_err());
}

#[test]
fn exact_fit_source_warns_instead_of_zooming_stretching_or_recropping() {
    let source = fixture(1.);
    let original = source.clone();
    let image = source.design.page_background.unwrap().image.unwrap().image;
    let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
    assert_eq!(
        background_photo_warning(&expanded, bleed),
        Some(BackgroundPhotoWarning::SourceTooSmall)
    );
    let NodeKind::Raster {
        raster: before,
        placement: old,
    } = &source.node(image).unwrap().kind
    else {
        panic!()
    };
    let NodeKind::Raster {
        raster: after,
        placement: new,
    } = &expanded.node(image).unwrap().kind
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(before, after));
    assert_eq!(
        *new,
        emulsion_raster::Placement {
            x: old.x + f64::from(bleed),
            y: old.y + f64::from(bleed),
            ..*old
        }
    );
    assert_trim_unchanged(&source, &expanded, bleed);
    let actual = flatten(&expanded.composite_tree(), 0);
    assert_eq!(actual.get(0, 0), [0, u16::MAX, 0, u16::MAX]);
    assert_eq!(source, original);
}

#[test]
fn partial_source_coverage_reveals_available_photo_and_warns_about_other_sides() {
    let mut source = fixture(1.);
    let image = source.design.page_background.unwrap().image.unwrap().image;
    let NodeKind::Raster { raster, placement } = &mut source.node_mut(image).unwrap().kind else {
        panic!()
    };
    *raster = Arc::new(Raster::solid(80, 30, [0.2, 0.3, 0.8, 1.]));
    placement.x = -20.;
    let expected_photo = raster.get(12, 15);
    let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
    assert_eq!(
        background_photo_warning(&expanded, bleed),
        Some(BackgroundPhotoWarning::SourceTooSmall)
    );
    let rendered = flatten(&expanded.composite_tree(), 0);
    assert_eq!(rendered.get(2, 25), expected_photo);
    assert_eq!(rendered.get(57, 25), expected_photo);
    assert_eq!(rendered.get(30, 2), [0, u16::MAX, 0, u16::MAX]);
    assert_trim_unchanged(&source, &expanded, bleed);
}

#[test]
fn rotated_flipped_crop_retains_pixels_transform_and_trim() {
    let mut source = fixture(4.);
    let image = source.design.page_background.unwrap().image.unwrap().image;
    let NodeKind::Raster { placement, .. } = &mut source.node_mut(image).unwrap().kind else {
        panic!()
    };
    placement.rotation = 31.;
    placement.flip_x = true;
    placement.flip_y = true;
    placement.x += 3.5;
    placement.y -= 2.25;
    let original = source.clone();
    let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
    assert_eq!(background_photo_warning(&expanded, bleed), None);
    let NodeKind::Raster {
        raster: before,
        placement: old,
    } = &source.node(image).unwrap().kind
    else {
        panic!()
    };
    let NodeKind::Raster {
        raster: after,
        placement: new,
    } = &expanded.node(image).unwrap().kind
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(before, after));
    assert_eq!(
        *new,
        emulsion_raster::Placement {
            x: old.x + f64::from(bleed),
            y: old.y + f64::from(bleed),
            ..*old
        }
    );
    assert_trim_unchanged(&source, &expanded, bleed);
    assert_eq!(source, original);
}

#[test]
fn rotated_source_bounding_box_does_not_falsely_claim_corner_coverage() {
    let mut source = fixture(1.6);
    let image = source.design.page_background.unwrap().image.unwrap().image;
    let NodeKind::Raster { placement, .. } = &mut source.node_mut(image).unwrap().kind else {
        panic!()
    };
    placement.rotation = 45.;
    let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
    let NodeKind::Raster { raster, placement } = &expanded.node(image).unwrap().kind else {
        panic!()
    };
    let bounds = placement.doc_bounds(raster.width(), raster.height());
    assert!(bounds.x <= 0 && bounds.y <= 0);
    assert!(bounds.right() >= expanded.width as i32 && bounds.bottom() >= expanded.height as i32);
    assert_eq!(
        background_photo_warning(&expanded, bleed),
        Some(BackgroundPhotoWarning::SourceTooSmall)
    );
}

#[test]
fn ordinary_native_image_frames_keep_their_authored_clip() {
    let mut source = fixture(2.);
    let frame = source.design.page_background.unwrap().image.unwrap();
    source.design.page_background.as_mut().unwrap().image = None;
    let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
    let NodeKind::Path { path: original, .. } = &source.node(frame.boundary).unwrap().kind else {
        panic!()
    };
    let NodeKind::Path { path: actual, .. } = &expanded.node(frame.boundary).unwrap().kind else {
        panic!()
    };
    let mut expected = (**original).clone();
    expected.translate(f64::from(bleed), f64::from(bleed));
    assert_eq!(**actual, expected);
    assert_eq!(background_photo_warning(&expanded, bleed), None);
    assert_trim_unchanged(&source, &expanded, bleed);
    assert_eq!(
        flatten(&expanded.composite_tree(), 0).get(2, 25),
        [0, u16::MAX, 0, u16::MAX]
    );
}

#[test]
fn custom_background_boundary_is_preserved_and_reported() {
    let mut source = fixture(2.);
    let frame = source.design.page_background.unwrap().image.unwrap();
    let NodeKind::Path { path, style, cache } = &mut source.node_mut(frame.boundary).unwrap().kind
    else {
        panic!()
    };
    *path = Arc::new(vector_geometry::ellipse(0., 0., 40., 30.));
    *cache = emulsion_core::vector_cache::VectorRaster::path(path.clone(), *style, 40, 30);
    let original = source.clone();
    let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
    assert_eq!(
        background_photo_warning(&expanded, bleed),
        Some(BackgroundPhotoWarning::CustomizedFrame)
    );
    assert_trim_unchanged(&source, &expanded, bleed);
    assert_eq!(source, original);
}

#[test]
fn transparent_photo_opacity_and_source_mask_are_preserved() {
    let mut source = fixture(2.);
    let frame = source.design.page_background.unwrap().image.unwrap();
    let node = source.node_mut(frame.image).unwrap();
    node.opacity = 0.5;
    node.mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
        40,
        30,
        0,
        |x, _| if x < 20 { 255 } else { 128 },
    )));
    let NodeKind::Raster { raster, .. } = &mut node.kind else {
        panic!()
    };
    *raster = Arc::new(Raster::solid(40, 30, [0.25, 0.1, 0.05, 0.5]));
    let original = source.clone();
    let (expanded, bleed) = with_bleed(&source, 2.54).unwrap();
    assert_eq!(
        background_photo_warning(&expanded, bleed),
        None,
        "Preflight checks source bounds, not opacity"
    );
    let after = expanded.node(frame.image).unwrap();
    assert_eq!(after.opacity, 0.5);
    assert!(Arc::ptr_eq(
        source.node(frame.image).unwrap().mask.as_ref().unwrap(),
        after.mask.as_ref().unwrap()
    ));
    assert_trim_unchanged(&source, &expanded, bleed);
    assert_eq!(source, original);
}

#[test]
fn hidden_backgrounds_and_color_only_pages_do_not_warn() {
    let source = fixture(1.);
    let frame = source.design.page_background.unwrap().image.unwrap();
    for id in [frame.group, frame.boundary, frame.image] {
        let mut hidden = source.clone();
        hidden.node_mut(id).unwrap().visible = false;
        let (expanded, bleed) = with_bleed(&hidden, 2.54).unwrap();
        assert_eq!(background_photo_warning(&expanded, bleed), None);
    }
    let mut editor = Editor::new(source, None);
    design_background::remove_image(&mut editor).unwrap();
    let (expanded, bleed) = with_bleed(&editor.doc, 2.54).unwrap();
    assert_eq!(background_photo_warning(&expanded, bleed), None);
}

#[test]
fn original_preflight_matches_expanded_geometry_without_modifying_source() {
    let mut cases = vec![("covered", fixture(2.)), ("exact fit", fixture(1.))];
    let mut partial = fixture(1.);
    let image = partial.design.page_background.unwrap().image.unwrap().image;
    let NodeKind::Raster { raster, placement } = &mut partial.node_mut(image).unwrap().kind else {
        panic!()
    };
    *raster = Arc::new(Raster::solid(80, 30, [0.2, 0.3, 0.8, 1.]));
    placement.x = -20.;
    cases.push(("partial", partial));
    for zoom in [1.6, 4.] {
        let mut rotated = fixture(zoom);
        let image = rotated.design.page_background.unwrap().image.unwrap().image;
        let NodeKind::Raster { placement, .. } = &mut rotated.node_mut(image).unwrap().kind else {
            panic!()
        };
        placement.rotation = 45.;
        placement.flip_x = true;
        placement.flip_y = true;
        cases.push(("rotated and flipped", rotated));
    }
    let mut custom = fixture(2.);
    let boundary = custom
        .design
        .page_background
        .unwrap()
        .image
        .unwrap()
        .boundary;
    let NodeKind::Path { path, style, cache } = &mut custom.node_mut(boundary).unwrap().kind else {
        panic!()
    };
    *path = Arc::new(vector_geometry::ellipse(0., 0., 40., 30.));
    *cache = emulsion_core::vector_cache::VectorRaster::path(path.clone(), *style, 40, 30);
    cases.push(("custom boundary", custom));
    for (name, source) in cases {
        let original = source.clone();
        for mm in [0., 0.01, 2.54] {
            let direct = original_background_photo_warning(&source, mm).unwrap();
            let (expanded, bleed) = with_bleed(&source, mm).unwrap();
            assert_eq!(
                direct,
                background_photo_warning(&expanded, bleed),
                "{name}: {mm} mm"
            );
            assert_eq!(source, original);
        }
    }
}

#[test]
fn original_preflight_does_not_load_missing_raw_or_render_unrelated_artwork() {
    let mut source = fixture(2.);
    let image = source.design.page_background.unwrap().image.unwrap().image;
    source.raw = Some(emulsion_core::raw::RawDocument {
        schema_version: 1,
        node_id: image,
        source: "/__emulsion_missing_bleed_source__/original.dng".into(),
        source_sha256: "0".repeat(64),
        params: Default::default(),
        metadata: Default::default(),
    });
    source.raw.as_ref().unwrap().validate().unwrap();
    // The saved photo geometry is sufficient even with an unavailable original.
    assert_eq!(
        original_background_photo_warning(&source, 2.54).unwrap(),
        None
    );
    source.design.page_background = None;
    let foreground = source
        .nodes
        .iter_mut()
        .find(|node| node.name == "Foreground marker")
        .unwrap();
    foreground.mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
        40,
        30,
        0,
        |x, _| if x < 20 { 255 } else { 0 },
    )));
    let original = source.clone();
    for _ in 0..32 {
        assert_eq!(
            original_background_photo_warning(&source, 2.54).unwrap(),
            None
        );
    }
    assert_eq!(source, original);
    for node in &source.nodes {
        if let NodeKind::Path { cache, .. } = &node.kind {
            assert!(
                !cache.is_rendered(),
                "Geometric preflight must not render artwork"
            );
        }
    }
    // Invalid unrelated RAW metadata is also outside geometric preflight's job.
    source.raw.as_mut().unwrap().schema_version = 0;
    assert_eq!(
        original_background_photo_warning(&source, 2.54).unwrap(),
        None
    );
}

#[test]
fn original_preflight_reuses_bleed_and_resolution_validation() {
    let source = fixture(2.);
    for bleed in [-1., f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(original_background_photo_warning(&source, bleed).is_err());
    }
    for resolution in [0., -100., f32::NAN, f32::INFINITY] {
        let mut source = source.clone();
        source.resolution = resolution;
        assert!(original_background_photo_warning(&source, 2.54).is_err());
    }
}
