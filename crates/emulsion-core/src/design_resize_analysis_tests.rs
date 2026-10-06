use super::*;
use crate::{
    command::Slot,
    design_metadata::{Anchor, Constraint},
    text::TextSpec,
};
use emulsion_raster::{Placement, Raster, vector::PathStyle};

fn add(editor: &mut Editor, node: Node) -> NodeId {
    editor
        .execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
}
fn shape(editor: &mut Editor, x: f64, y: f64, w: f64, h: f64) -> NodeId {
    let (width, height) = (editor.doc.width, editor.doc.height);
    add(
        editor,
        Node::path(
            0,
            "Artwork",
            Arc::new(vector_geometry::rectangle(x, y, w, h)),
            PathStyle {
                fill: Some([255; 4]),
                stroke: None,
                ..Default::default()
            },
            width,
            height,
        ),
    )
}
#[test]
fn named_targets_use_shared_physical_conversion_and_explicit_units() {
    let expected = [
        (1080, 1080),
        (1080, 1350),
        (1080, 1920),
        (1920, 1080),
        (2480, 3508),
        (2550, 3300),
    ];
    for (preset, size) in PRESETS.iter().zip(expected) {
        assert_eq!(preset.pixel_size().unwrap(), size);
        assert!(preset.dimensions_label().contains(preset.unit.label()));
        assert!(preset.dimensions_label().contains("ppi"));
    }
    assert_eq!(PRESETS[4].unit, Unit::Millimeters);
    assert_eq!(PRESETS[5].unit, Unit::Inches);
    let source = Document::new(200, 200);
    let plan = prepare(&source, 2480, 3508, 300.).unwrap();
    assert_eq!(plan.doc.resolution, 300.);
    assert_eq!(source.resolution, 72.);
    for resolution in [0., f32::NAN, f32::INFINITY, 9601.] {
        assert!(prepare(&source, 200, 200, resolution).is_err());
    }
    assert!(prepare(&source, 0, 200, 72.).is_err());
    assert!(prepare(&source, 30_000, 30_000, 72.).is_err());
}
#[test]
fn pinned_locked_rotated_background_covers_every_target_without_resampling() {
    let mut editor = Editor::new(Document::new(400, 300), None);
    let pixels = Arc::new(Raster::solid(160, 40, [0.1, 0.4, 0.8, 1.]));
    let image = add(
        &mut editor,
        Node::raster(
            0,
            "Photo",
            pixels.clone(),
            Placement {
                rotation: 31.,
                flip_x: true,
                flip_y: true,
                ..Default::default()
            },
        ),
    );
    crate::design_background::set_image(&mut editor, image).unwrap();
    let crop = crate::design::crop_frame_image(&editor.doc, image, [12., -8.], 1.7).unwrap();
    editor.execute(crop).unwrap();
    let background = crate::design_background::parts(&editor.doc).unwrap();
    let frame = background.image.unwrap();
    for id in [background.fill, frame.group, frame.boundary, frame.image] {
        let node = editor.doc.node_mut(id).unwrap();
        node.locked = true;
        node.locks = crate::node::LayerLocks {
            pixels: true,
            position: true,
            transparency: true,
        };
    }
    // A background's role wins over obsolete authored anchor rules.
    editor.doc.design.constraints.insert(
        frame.group,
        Constraint {
            horizontal: Anchor::Start,
            vertical: Anchor::Start,
            reflow_text: false,
        },
    );
    let source = editor.doc.clone();
    for (width, height) in [(1080, 1080), (1080, 1920), (1920, 1080), (2480, 3508)] {
        let plan = prepare(&source, width, height, 300.).unwrap();
        assert_eq!(crate::design_background::parts(&plan.doc), Some(background));
        assert_eq!(plan.doc.children(None)[..2], [background.fill, frame.group]);
        assert_eq!(image_covers_frame(&plan.doc, image), Some(true));
        assert!(plan.photo_coverage.is_empty());
        assert!(plan.overflow.is_empty());
        assert!(
            crate::design_metadata::resize_variant(&source, width, height)
                .unwrap()
                .overflow
                .is_empty()
        );
        assert!(plan.background_recropped);
        let NodeKind::Raster { raster, placement } = &plan.doc.node(image).unwrap().kind else {
            panic!()
        };
        assert!(Arc::ptr_eq(raster, &pixels));
        assert_eq!(placement.rotation, 31.);
        assert!(placement.flip_x && placement.flip_y);
        assert!((placement.scale_x - placement.scale_y).abs() < 1e-9);
        for id in [background.fill, frame.group, frame.boundary, frame.image] {
            assert!(plan.doc.node(id).unwrap().locked);
            assert_eq!(
                plan.doc.node(id).unwrap().locks,
                source.node(id).unwrap().locks
            );
        }
        plan.doc.validate().unwrap();
    }
    assert_eq!(editor.doc, source);
}
#[test]
fn locked_foreground_is_rejected_but_unchanged_anchored_foreground_is_allowed() {
    let mut editor = Editor::new(Document::new(200, 200), None);
    let id = shape(&mut editor, 20., 20., 50., 50.);
    editor.doc.node_mut(id).unwrap().locked = true;
    let before = editor.doc.clone();
    assert!(prepare(&before, 300, 300, 72.).is_err());
    assert_eq!(editor.doc, before);
    editor.doc.design.constraints.insert(
        id,
        Constraint {
            horizontal: Anchor::Start,
            vertical: Anchor::Start,
            reflow_text: false,
        },
    );
    let plan = prepare(&editor.doc, 300, 300, 72.).unwrap();
    assert_eq!(plan.doc.node(id), editor.doc.node(id));
    editor.doc.node_mut(id).unwrap().locked = false;
    editor.doc.node_mut(id).unwrap().locks.position = true;
    editor.doc.design.constraints.remove(&id);
    assert!(prepare(&editor.doc, 300, 300, 72.).is_err());
}
#[test]
fn reflow_reports_changed_breaks_and_preclip_text_overflow_without_altering_style() {
    let mut editor = Editor::new(Document::new(600, 300), None);
    let font = crate::design_fonts::EmbeddedFont::from_bytes(
        include_bytes!("../../../assets/fonts/Geist.ttf").to_vec(),
    )
    .unwrap();
    let id=add(&mut editor,Node::text(0,"Headline",TextSpec {text:"Native editable text wraps onto additional lines when the available paragraph becomes narrow".into(),font:font.alias().into(),size:24.,width:Some(560.),height:Some(65.),x:10.,y:10.,bold:true,color:[10,30,50,255],..Default::default()},600,300));
    editor.doc.design.fonts.insert(font.alias().into(), font);
    editor.doc.design.saved_styles.insert(
        "Heading".into(),
        crate::design_styles::SavedStyle {
            appearance: crate::design_appearance::Appearance::capture(editor.doc.node(id).unwrap()),
        },
    );
    editor.doc.design.style_links.insert(id, "Heading".into());
    editor.doc.design.constraints.insert(
        id,
        Constraint {
            horizontal: Anchor::Stretch,
            vertical: Anchor::Start,
            reflow_text: true,
        },
    );
    editor.doc.design.speaker_notes = "Keep speaker notes".into();
    editor.doc.design.interactions.insert(
        id,
        vec![crate::design_interactions::Action::Slide { page: 42 }],
    );
    let source = editor.doc.clone();
    let plan = prepare(&source, 200, 300, 72.).unwrap();
    assert!(plan.text_reflow.contains(&id));
    assert!(plan.text_overflow.contains(&id));
    assert_eq!(plan.doc.design, source.design);
    let NodeKind::Text { spec: old, .. } = &source.node(id).unwrap().kind else {
        panic!()
    };
    let NodeKind::Text { spec: new, .. } = &plan.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(
        (new.size, new.scale_x, new.scale_y),
        (old.size, old.scale_x, old.scale_y)
    );
    assert_eq!(new.font, old.font);
    assert_eq!(new.text, old.text);
    assert_eq!(new.runs, old.runs);
    assert_eq!(new.height, old.height);
    assert_eq!(new.width, Some(160.));
    assert!(crate::text::layout(new).bounds().height <= 65.);
    assert_eq!(editor.doc, source);
}
#[test]
fn line_break_analysis_ignores_pure_visual_scaling_and_checks_vertical_columns() {
    let mut editor = Editor::new(Document::new(200, 200), None);
    let id = add(
        &mut editor,
        Node::text(
            0,
            "Text",
            TextSpec {
                text: "Some editable text".into(),
                size: 16.,
                width: Some(180.),
                ..Default::default()
            },
            200,
            200,
        ),
    );
    let plan = prepare(&editor.doc, 400, 400, 72.).unwrap();
    assert!(!plan.text_reflow.contains(&id));
    let vertical = TextSpec {
        text: "ABCDEFGHIJK".into(),
        size: 20.,
        vertical: true,
        width: Some(22.),
        height: Some(45.),
        ..Default::default()
    };
    assert!(crate::text::resize_flow(&vertical).unwrap().1);
}
#[test]
fn analysis_warns_on_contain_gaps_and_does_not_call_cover_overhang_page_overflow() {
    let mut editor = Editor::new(Document::new(600, 400), None);
    let group = crate::design::frame(&editor.doc, crate::design::Element::Rectangle)
        .paste(&mut editor, Slot::TOP, (0., 0.))
        .unwrap()[0];
    let image = crate::design::place_in_frame(
        &mut editor,
        group,
        Arc::new(Raster::solid(400, 100, [0.2, 0.3, 0.4, 1.])),
    )
    .unwrap();
    let plan = prepare(&editor.doc, 600, 400, 72.).unwrap();
    assert!(!plan.overflow.contains(&image));
    assert!(!plan.photo_coverage.contains(&image));
    editor
        .execute(
            crate::design::fit_frame_image(
                &editor.doc,
                image,
                crate::design::ImageFit::Contain,
                [0.5; 2],
            )
            .unwrap(),
        )
        .unwrap();
    let plan = prepare(&editor.doc, 600, 400, 72.).unwrap();
    assert_eq!(plan.photo_coverage, vec![image]);
}
#[test]
fn visible_offpage_content_is_reported_hidden_content_ignored_and_warp_is_unchecked() {
    let mut editor = Editor::new(Document::new(200, 200), None);
    let outside = shape(&mut editor, 180., 10., 50., 50.);
    let hidden = shape(&mut editor, -50., 10., 20., 20.);
    editor.doc.node_mut(hidden).unwrap().visible = false;
    let text = add(
        &mut editor,
        Node::text(
            0,
            "Warp",
            TextSpec {
                text: "Warp text".into(),
                size: 16.,
                warp: crate::text_effects::TextWarp {
                    style: crate::text_effects::WarpStyle::Arc,
                    bend: 40.,
                    ..Default::default()
                },
                ..Default::default()
            },
            200,
            200,
        ),
    );
    let plan = prepare(&editor.doc, 200, 200, 72.).unwrap();
    assert!(plan.overflow.contains(&outside));
    assert!(!plan.overflow.contains(&hidden));
    assert!(plan.unchecked.contains(&text));
}
#[test]
fn resize_keeps_movement_links_without_double_moving_and_repeated_previews_are_source_based() {
    let mut editor = Editor::new(Document::new(200, 200), None);
    let a = shape(&mut editor, 10., 10., 30., 30.);
    let b = shape(&mut editor, 60., 60., 30., 30.);
    editor
        .execute(Command::SetLayerLinks {
            ids: vec![a, b],
            linked: true,
        })
        .unwrap();
    let source = editor.doc.clone();
    let first = prepare(&source, 300, 400, 72.).unwrap();
    let second = prepare(&source, 300, 400, 72.).unwrap();
    assert_eq!(first.doc, second.doc);
    for id in [a, b] {
        assert_eq!(
            first.doc.node(id).unwrap().link_group,
            source.node(id).unwrap().link_group
        );
    }
    assert_eq!(
        crate::geometry::node_bounds(&first.doc, a)
            .unwrap()
            .unwrap()
            .x,
        15
    );
    assert_eq!(
        crate::geometry::node_bounds(&first.doc, b)
            .unwrap()
            .unwrap()
            .x,
        90
    );
    assert_eq!(editor.doc, source);
}
#[test]
fn final_lock_validation_does_not_allow_background_pixel_or_style_replacement() {
    let mut editor = Editor::new(Document::new(200, 200), None);
    let image = crate::design_background::replace_image(
        &mut editor,
        Arc::new(Raster::solid(40, 20, [1.; 4])),
    )
    .unwrap();
    editor.doc.node_mut(image).unwrap().locked = true;
    let source = editor.doc.clone();
    let mut next = source.clone();
    let NodeKind::Raster { raster, .. } = &mut next.node_mut(image).unwrap().kind else {
        panic!()
    };
    *raster = Arc::new(Raster::solid(40, 20, [0.5; 4]));
    assert!(validate_resize_locks(&source, &next).is_err());
    let mut next = source.clone();
    next.node_mut(image).unwrap().opacity = 0.5;
    assert!(validate_resize_locks(&source, &next).is_err());
}
#[test]
fn rotated_covered_photo_frame_resizes_without_skew_or_source_pixel_changes() {
    let mut editor = Editor::new(Document::new(600, 400), None);
    let group = crate::design::frame(&editor.doc, crate::design::Element::Rectangle)
        .paste(&mut editor, Slot::TOP, (0., 0.))
        .unwrap()[0];
    let pixels = Arc::new(Raster::solid(400, 100, [0.2, 0.3, 0.4, 1.]));
    let image = crate::design::place_in_frame(&mut editor, group, pixels.clone()).unwrap();
    let NodeKind::Raster { placement, .. } = &mut editor.doc.node_mut(image).unwrap().kind else {
        panic!()
    };
    placement.rotation = 27.;
    placement.flip_x = true;
    editor
        .execute(
            crate::design::fit_frame_image(
                &editor.doc,
                image,
                crate::design::ImageFit::Cover,
                [0.65, 0.3],
            )
            .unwrap(),
        )
        .unwrap();
    let source = editor.doc.clone();
    let plan = prepare(&source, 300, 800, 72.).unwrap();
    assert_eq!(image_covers_frame(&plan.doc, image), Some(true));
    let NodeKind::Raster { raster, placement } = &plan.doc.node(image).unwrap().kind else {
        panic!()
    };
    assert!(Arc::ptr_eq(raster, &pixels));
    assert_eq!(placement.rotation, 27.);
    assert!(placement.flip_x);
    assert!((placement.scale_x - placement.scale_y).abs() < 1e-9);
    assert_eq!(editor.doc, source);
}
#[test]
fn cropped_photo_frame_preserves_offcentre_focus_zoom_and_editable_border() {
    let mut editor = Editor::new(Document::new(600, 400), None);
    let group = crate::design::frame(&editor.doc, crate::design::Element::Rectangle)
        .paste(&mut editor, Slot::TOP, (0., 0.))
        .unwrap()[0];
    let boundary = crate::design::frame_parts(&editor.doc, group).unwrap().0;
    let pixels = Arc::new(Raster::solid(400, 100, [0.2, 0.3, 0.4, 1.]));
    let image = crate::design::place_in_frame(&mut editor, group, pixels.clone()).unwrap();
    let NodeKind::Path { path, .. } = &editor.doc.node(boundary).unwrap().kind else {
        panic!()
    };
    let border = editor
        .execute(Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Border",
                path.clone(),
                PathStyle {
                    fill: None,
                    stroke: Some([0, 0, 0, 255]),
                    width: 3.,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::top_of(Some(group)),
        })
        .unwrap()
        .unwrap();
    editor
        .execute(Command::SetClip {
            id: border,
            clip_to: Some(boundary),
        })
        .unwrap();
    editor
        .execute(crate::design::crop_frame_image(&editor.doc, image, [30., -15.], 2.4).unwrap())
        .unwrap();
    let source = editor.doc.clone();
    let old = crop_parameters(&source, boundary, image).unwrap();
    let plan = prepare(&source, 1200, 800, 72.).unwrap();
    let new = crop_parameters(&plan.doc, boundary, image).unwrap();
    for axis in 0..2 {
        assert!((old.0[axis] - new.0[axis]).abs() < 1e-8, "{old:?} {new:?}");
    }
    assert!((old.1 - new.1).abs() < 1e-8);
    assert_eq!(plan.doc.node(border).unwrap().clip_to, Some(boundary));
    let NodeKind::Path {
        path: boundary_path,
        ..
    } = &plan.doc.node(boundary).unwrap().kind
    else {
        panic!()
    };
    let NodeKind::Path {
        path: border_path,
        style,
        ..
    } = &plan.doc.node(border).unwrap().kind
    else {
        panic!()
    };
    assert_eq!(border_path, boundary_path);
    assert_eq!(style.stroke, Some([0, 0, 0, 255]));
    assert_eq!(editor.doc, source);
}
#[test]
fn text_clipped_by_a_native_boundary_reports_overflow_and_visual_check() {
    let mut editor = Editor::new(Document::new(300, 200), None);
    let boundary = shape(&mut editor, 0., 0., 30., 30.);
    let text = add(
        &mut editor,
        Node::text(
            0,
            "Clipped headline",
            TextSpec {
                text: "Much wider than its clipping shape".into(),
                size: 24.,
                ..Default::default()
            },
            300,
            200,
        ),
    );
    editor
        .execute(Command::SetClip {
            id: text,
            clip_to: Some(boundary),
        })
        .unwrap();
    let plan = prepare(&editor.doc, 300, 200, 72.).unwrap();
    assert!(plan.text_overflow.contains(&text));
    assert!(plan.unchecked.contains(&text));
}
#[test]
fn unchanged_target_keeps_locked_mask_identity_and_creates_no_geometry_changes() {
    let mut editor = Editor::new(Document::new(200, 200), None);
    let id = shape(&mut editor, 20., 20., 40., 40.);
    editor
        .execute(Command::SetMask {
            id,
            mask: Some(Arc::new(emulsion_raster::Mask::from_fn(
                200,
                200,
                0,
                |_, _| 255,
            ))),
        })
        .unwrap();
    editor.doc.node_mut(id).unwrap().locked = true;
    let source = editor.doc.clone();
    let plan = prepare(&source, 200, 200, 72.).unwrap();
    assert_eq!(plan.doc, source);
    assert!(Arc::ptr_eq(
        plan.doc.node(id).unwrap().mask.as_ref().unwrap(),
        source.node(id).unwrap().mask.as_ref().unwrap()
    ));
}
#[test]
fn nested_cover_photos_resize_without_distortion_and_unsupported_masked_groups_reject() {
    for rotation in [0., 27.] {
        let mut editor = Editor::new(Document::new(600, 400), None);
        let frame = crate::design::frame(&editor.doc, crate::design::Element::Rectangle)
            .paste(&mut editor, Slot::TOP, (0., 0.))
            .unwrap()[0];
        let image = crate::design::place_in_frame(
            &mut editor,
            frame,
            Arc::new(Raster::solid(400, 100, [0.2, 0.3, 0.4, 1.])),
        )
        .unwrap();
        let NodeKind::Raster { placement, .. } = &mut editor.doc.node_mut(image).unwrap().kind
        else {
            panic!()
        };
        placement.rotation = rotation;
        editor
            .execute(
                crate::design::fit_frame_image(
                    &editor.doc,
                    image,
                    crate::design::ImageFit::Cover,
                    [0.5; 2],
                )
                .unwrap(),
            )
            .unwrap();
        let text = add(
            &mut editor,
            Node::text(
                0,
                "Caption",
                TextSpec {
                    text: "Grouped photo".into(),
                    size: 20.,
                    x: 10.,
                    y: 300.,
                    ..Default::default()
                },
                600,
                400,
            ),
        );
        let group = editor
            .execute(Command::Group {
                ids: vec![frame, text],
                name: "Photo card".into(),
            })
            .unwrap()
            .unwrap();
        let source = editor.doc.clone();
        let nested = prepare(&source, 300, 800, 72.).unwrap();
        assert_eq!(image_covers_frame(&nested.doc, image), Some(true));
        assert_eq!(nested.doc.node(frame).unwrap().parent, Some(group));
        let NodeKind::Raster { placement, .. } = &nested.doc.node(image).unwrap().kind else {
            panic!()
        };
        assert!((placement.scale_x - placement.scale_y).abs() < 1e-9);
        assert_eq!(placement.rotation, rotation);
        let mut masked = source.clone();
        masked.node_mut(group).unwrap().mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
            600,
            400,
            0,
            |_, _| 255,
        )));
        let error = prepare(&masked, 300, 800, 72.)
            .err()
            .expect("A masked photo group cannot be resized nonuniformly safely");
        assert!(error.contains("proportional"));
        assert_eq!(editor.doc, source);
        let proportional = prepare(&source, 1200, 800, 72.).unwrap();
        assert_eq!(proportional.doc.node(frame).unwrap().parent, Some(group));
        assert_eq!(proportional.doc.node(text).unwrap().parent, Some(group));
        assert_eq!(image_covers_frame(&proportional.doc, image), Some(true));
    }
}
#[test]
fn baseline_shifted_rich_text_is_explicitly_unchecked_instead_of_false_clean() {
    let mut editor = Editor::new(Document::new(300, 200), None);
    let text = add(
        &mut editor,
        Node::text(
            0,
            "Raised text",
            TextSpec {
                text: "Raised".into(),
                size: 24.,
                width: Some(200.),
                height: Some(30.),
                runs: vec![crate::text::TextRun {
                    start: 0,
                    end: 6,
                    style: crate::text::TextStyle {
                        size: 24.,
                        baseline: -40.,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            300,
            200,
        ),
    );
    let plan = prepare(&editor.doc, 300, 200, 72.).unwrap();
    assert!(plan.unchecked.contains(&text));
}
