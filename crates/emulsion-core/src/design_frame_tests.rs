use super::*;
use crate::{Editor, NodeId, node::LayerLocks};
use emulsion_raster::{Placement, Raster};
use glam::dvec2;

fn fixture(with_image: bool) -> (Editor, NodeId, NodeId, Option<NodeId>) {
    let mut editor = Editor::new(Document::new(600, 400), None);
    let group = frame(&editor.doc, Element::Rectangle)
        .paste(&mut editor, Slot::TOP, (0., 0.))
        .unwrap()[0];
    let boundary = frame_parts(&editor.doc, group).unwrap().0;
    let image = with_image.then(|| {
        place_in_frame(
            &mut editor,
            group,
            Arc::new(Raster::solid(400, 100, [0.2, 0.3, 0.4, 1.])),
        )
        .unwrap()
    });
    (editor, group, boundary, image)
}

fn media(doc: &Document, id: NodeId) -> (&Arc<Raster>, Placement) {
    let NodeKind::Raster { raster, placement } = &doc.node(id).unwrap().kind else {
        panic!("Expected editable frame pixels")
    };
    (raster, *placement)
}

fn assert_covered(doc: &Document, boundary: NodeId, image: NodeId) {
    let (raster, placement) = media(doc, image);
    let inverse = placement.to_doc(raster.width(), raster.height()).inverse();
    let NodeKind::Path { path, .. } = &doc.node(boundary).unwrap().kind else {
        panic!("Expected native frame boundary")
    };
    for point in path
        .subpaths
        .iter()
        .flat_map(|sub| &sub.anchors)
        .map(|a| inverse.transform_point2(dvec2(a.p.0, a.p.1)))
    {
        assert!(point.x >= -1e-7 && point.x <= f64::from(raster.width()) + 1e-7);
        assert!(point.y >= -1e-7 && point.y <= f64::from(raster.height()) + 1e-7);
    }
}

#[test]
fn first_insertion_and_replacement_keep_editable_border_and_one_undo() {
    let (mut editor, group, boundary, _) = fixture(false);
    let NodeKind::Path { path, .. } = &editor.doc.node(boundary).unwrap().kind else {
        panic!("Expected frame boundary")
    };
    let border = Node::path(
        0,
        "Editable border",
        path.clone(),
        PathStyle {
            fill: None,
            stroke: Some([12, 34, 56, 255]),
            width: 3.,
            ..Default::default()
        },
        600,
        400,
    );
    let border = editor
        .execute(Command::AddNode {
            node: Box::new(border),
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
    let before = editor.doc.clone();
    let history = editor.history.len();
    let image = place_in_frame(
        &mut editor,
        group,
        Arc::new(Raster::solid(200, 100, [0.2, 0.3, 0.4, 1.])),
    )
    .unwrap();
    assert_eq!(editor.history.len(), history + 1);
    assert_eq!(
        editor.doc.children(Some(group)),
        vec![boundary, image, border]
    );
    assert_eq!(editor.doc.node(border), before.node(border));
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(editor.redo());
    let inserted = editor.doc.clone();
    assert_eq!(
        place_in_frame(
            &mut editor,
            group,
            Arc::new(Raster::solid(30, 90, [0.7, 0.2, 0.1, 1.])),
        )
        .unwrap(),
        image
    );
    assert_eq!(
        editor.doc.children(Some(group)),
        vec![boundary, image, border]
    );
    assert_eq!(editor.doc.node(border), before.node(border));
    assert!(editor.undo());
    assert_eq!(editor.doc, inserted);
}

#[test]
fn repeated_replacement_preserves_rotated_flipped_frame_identity_and_single_undo() {
    let (mut editor, group, boundary, image) = fixture(true);
    let image = image.unwrap();
    let mut placement = media(&editor.doc, image).1;
    placement.flip_x = true;
    placement.flip_y = true;
    editor
        .execute(Command::SetPlacement {
            id: image,
            placement,
        })
        .unwrap();
    editor
        .execute(Command::RotateNode {
            id: group,
            degrees: 37.,
        })
        .unwrap();
    let frame = editor.doc.node(boundary).unwrap().clone();
    let group_node = editor.doc.node(group).unwrap().clone();
    let angle = media(&editor.doc, image).1.rotation;
    let count = editor.doc.nodes.len();
    for (w, h) in [(30, 90), (800, 200), (40, 40), (100, 300)] {
        editor
            .execute(crop_frame_image(&editor.doc, group, [8., -5.], 1.5).unwrap())
            .unwrap();
        let before = editor.doc.clone();
        let history = editor.history.len();
        let pixels = Arc::new(Raster::solid(w, h, [0.7, 0.2, 0.1, 1.]));
        assert_eq!(
            place_in_frame(&mut editor, group, pixels.clone()).unwrap(),
            image
        );
        assert_eq!(editor.doc.nodes.len(), count);
        assert_eq!(editor.history.len(), history + 1);
        assert_eq!(editor.doc.node(boundary), Some(&frame));
        assert_eq!(editor.doc.node(group), Some(&group_node));
        assert_eq!(editor.doc.node(image).unwrap().clip_to, Some(boundary));
        assert_eq!(editor.doc.node(image).unwrap().parent, Some(group));
        let (source, replacement) = media(&editor.doc, image);
        assert!(Arc::ptr_eq(source, &pixels));
        assert_eq!(replacement.rotation, angle);
        assert_eq!((replacement.flip_x, replacement.flip_y), (true, true));
        assert_covered(&editor.doc, boundary, image);
        let after = editor.doc.clone();
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(editor.redo());
        assert_eq!(editor.doc, after);
    }
}

#[test]
fn replacement_rejects_locked_empty_and_populated_frames_without_any_edit() {
    for with_image in [false, true] {
        for target in ["boundary", "group", "ancestor", "image"] {
            if target == "image" && !with_image {
                continue;
            }
            for lock in ["full", "position", "pixels", "transparency"] {
                let (mut editor, group, boundary, image) = fixture(with_image);
                let ancestor = editor
                    .execute(Command::Group {
                        ids: vec![group],
                        name: "Containing design".into(),
                    })
                    .unwrap()
                    .unwrap();
                let id = match target {
                    "boundary" => boundary,
                    "group" => group,
                    "ancestor" => ancestor,
                    _ => image.unwrap(),
                };
                let node = editor.doc.node_mut(id).unwrap();
                match lock {
                    "full" => node.locked = true,
                    "position" => node.locks.position = true,
                    "pixels" => node.locks.pixels = true,
                    _ => node.locks.transparency = true,
                }
                let before = editor.doc.clone();
                let history = editor.history.len();
                let revision = editor.revision;
                assert!(frame_image_replaceable(&editor.doc, group).is_err());
                assert!(
                    place_in_frame(
                        &mut editor,
                        group,
                        Arc::new(Raster::solid(80, 120, [0.7, 0.2, 0.1, 1.])),
                    )
                    .is_err(),
                    "{target} {lock}, image: {with_image}"
                );
                assert_eq!(editor.doc, before);
                assert_eq!(editor.history.len(), history);
                assert_eq!(editor.revision, revision);
                assert!(!editor.in_transaction());
            }
        }
    }
}

#[test]
fn crop_previews_cancel_losslessly_and_commit_as_one_undo_step() {
    let (mut editor, group, boundary, image) = fixture(true);
    let image = image.unwrap();
    let before = editor.doc.clone();
    let pixels = media(&before, image).0;
    let history = editor.history.len();
    let revision = editor.revision;
    let mut final_placement = None;
    for i in 1..=24 {
        let command =
            crop_frame_image(&before, group, [i as f64, -8.], 1. + i as f64 / 12.).unwrap();
        let mut preview = before.clone();
        command.apply(&mut preview).unwrap();
        assert!(Arc::ptr_eq(media(&preview, image).0, pixels));
        assert_eq!(preview.node(boundary), before.node(boundary));
        assert_eq!(preview.node(image).unwrap().clip_to, Some(boundary));
        assert_covered(&preview, boundary, image);
        final_placement = Some(media(&preview, image).1);
    }
    // Discarding any preview (Cancel/Escape) has no live document or history effect.
    assert_eq!(editor.doc, before);
    assert_eq!(editor.history.len(), history);
    assert_eq!(editor.revision, revision);
    frame_image_editable(&editor.doc, group).unwrap();
    editor
        .execute(Command::SetPlacement {
            id: image,
            placement: final_placement.unwrap(),
        })
        .unwrap();
    assert_eq!(editor.history.len(), history + 1);
    assert_ne!(editor.doc, before);
    assert!(Arc::ptr_eq(media(&editor.doc, image).0, pixels));
    assert_eq!(editor.doc.node(boundary), before.node(boundary));
    let after = editor.doc.clone();
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(editor.redo());
    assert_eq!(editor.doc, after);
}

#[test]
fn crop_clamps_pan_and_zoom_in_rotated_flipped_image_axes() {
    for flip_x in [false, true] {
        for flip_y in [false, true] {
            let (mut editor, group, boundary, image) = fixture(true);
            let image = image.unwrap();
            let mut placement = media(&editor.doc, image).1;
            placement.flip_x = flip_x;
            placement.flip_y = flip_y;
            editor
                .execute(Command::SetPlacement {
                    id: image,
                    placement,
                })
                .unwrap();
            editor
                .execute(Command::RotateNode {
                    id: group,
                    degrees: -53.,
                })
                .unwrap();
            let before = editor.doc.clone();
            let previous = media(&before, image).1;
            for (pan, zoom) in [
                ([1e4, -1e4], 2.5),
                ([-1e4, 1e4], 1.5),
                ([50., 50.], 0.01),
                ([0., 0.], 1.),
            ] {
                let command = crop_frame_image(&before, image, pan, zoom).unwrap();
                let mut preview = before.clone();
                command.apply(&mut preview).unwrap();
                let (pixels, crop) = media(&preview, image);
                assert!(Arc::ptr_eq(pixels, media(&before, image).0));
                assert_eq!(crop.rotation, previous.rotation);
                assert_eq!((crop.flip_x, crop.flip_y), (flip_x, flip_y));
                assert_eq!(preview.node(boundary), before.node(boundary));
                assert_covered(&preview, boundary, image);
            }
        }
    }
}

#[test]
fn crop_validates_inputs_and_locks_without_restricting_pixel_only_locks() {
    let (mut editor, group, boundary, image) = fixture(true);
    let image = image.unwrap();
    let before = editor.doc.clone();
    for (pan, zoom) in [
        ([f64::NAN, 0.], 1.),
        ([0., f64::INFINITY], 1.),
        ([0., 0.], f64::NAN),
        ([0., 0.], f64::INFINITY),
        ([0., 0.], 0.),
        ([0., 0.], -1.),
        ([0., 0.], f64::MAX),
    ] {
        assert!(crop_frame_image(&editor.doc, group, pan, zoom).is_err());
        assert_eq!(editor.doc, before);
    }
    for target in [group, boundary, image] {
        for position_only in [false, true] {
            let mut locked = before.clone();
            let node = locked.node_mut(target).unwrap();
            node.locked = !position_only;
            node.locks.position = position_only;
            assert!(frame_image_editable(&locked, group).is_err());
            assert!(crop_frame_image(&locked, group, [0., 0.], 2.).is_err());
            assert!(fit_frame_image(&locked, group, ImageFit::Cover, [0.5; 2]).is_err());
        }
    }
    editor.doc.node_mut(group).unwrap().locks = LayerLocks {
        pixels: true,
        transparency: true,
        ..Default::default()
    };
    assert!(frame_image_replaceable(&editor.doc, group).is_err());
    editor
        .execute(crop_frame_image(&editor.doc, group, [3., 0.], 2.).unwrap())
        .unwrap();
    assert!(Arc::ptr_eq(
        media(&editor.doc, image).0,
        media(&before, image).0
    ));
    let (empty, group, _, _) = fixture(false);
    assert!(frame_image_editable(&empty.doc, group).is_ok());
    assert!(crop_frame_image(&empty.doc, group, [0., 0.], 1.).is_err());
}
