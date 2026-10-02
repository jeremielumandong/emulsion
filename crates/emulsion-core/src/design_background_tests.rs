use super::*;
use crate::{design::crop_frame_image, fragment::Fragment, node::LayerLocks};
use emulsion_raster::{Placement, composite::flatten};
use glam::dvec2;

fn add(editor: &mut Editor, node: Node) -> NodeId {
    editor
        .execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
}

fn fixture() -> (Editor, NodeId, Arc<Raster>, NodeId) {
    let mut editor = Editor::new(Document::new(80, 60), None);
    let pixels = Arc::new(Raster::solid(160, 40, [0.1, 0.4, 0.8, 1.]));
    let image = add(
        &mut editor,
        Node::raster(
            0,
            "Original photo",
            pixels.clone(),
            Placement {
                x: 12.,
                y: 17.,
                rotation: 31.,
                flip_x: true,
                flip_y: true,
                ..Default::default()
            },
        ),
    );
    let foreground = add(
        &mut editor,
        Node::path(
            0,
            "Foreground",
            Arc::new(vector_geometry::rectangle(20., 20., 20., 10.)),
            PathStyle {
                fill: Some([220, 20, 50, 255]),
                stroke: None,
                ..Default::default()
            },
            80,
            60,
        ),
    );
    (editor, image, pixels, foreground)
}

fn assert_covered(doc: &Document, image: NodeId) {
    let NodeKind::Raster { raster, placement } = &doc.node(image).unwrap().kind else {
        panic!()
    };
    let inverse = placement.to_doc(raster.width(), raster.height()).inverse();
    for point in [
        [0., 0.],
        [f64::from(doc.width), 0.],
        [0., f64::from(doc.height)],
        [f64::from(doc.width), f64::from(doc.height)],
    ] {
        let point = inverse.transform_point2(dvec2(point[0], point[1]));
        assert!(
            point.x >= -1e-7 && point.x <= f64::from(raster.width()) + 1e-7,
            "{point:?}"
        );
        assert!(
            point.y >= -1e-7 && point.y <= f64::from(raster.height()) + 1e-7,
            "{point:?}"
        );
    }
}

#[test]
fn selected_source_keeps_identity_pixels_angle_flips_and_one_undo() {
    let (mut editor, image, pixels, foreground) = fixture();
    let before = editor.doc.clone();
    let history = editor.history.len();
    assert_eq!(set_image(&mut editor, image).unwrap(), image);
    assert_eq!(editor.history.len(), history + 1);
    let background = parts(&editor.doc).unwrap();
    let frame = background.image.unwrap();
    assert_eq!(
        editor.doc.children(None),
        vec![background.fill, frame.group, foreground]
    );
    assert_eq!(
        crate::design::frame_parts(&editor.doc, frame.group),
        Some((frame.boundary, Some(image)))
    );
    let NodeKind::Raster { raster, placement } = &editor.doc.node(image).unwrap().kind else {
        panic!()
    };
    assert!(Arc::ptr_eq(raster, &pixels));
    assert_eq!(placement.rotation, 31.);
    assert!(placement.flip_x && placement.flip_y);
    assert_eq!(placement.scale_x, placement.scale_y);
    assert_eq!(editor.doc.node(foreground), before.node(foreground));
    assert_covered(&editor.doc, image);
    let after = editor.doc.clone();
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(editor.redo());
    assert_eq!(editor.doc, after);
}

#[test]
fn legacy_fill_is_adopted_by_structure_and_color_and_remove_are_independent() {
    let mut editor = Editor::new(Document::new(80, 60), None);
    let fill = add(
        &mut editor,
        Node::new(
            0,
            "A user-renamed template layer",
            NodeKind::Fill {
                rgba: [30, 40, 50, 255],
            },
        ),
    );
    let foreground = add(
        &mut editor,
        Node::path(
            0,
            "Decoration",
            Arc::new(vector_geometry::rectangle(20., 20., 20., 10.)),
            PathStyle::default(),
            80,
            60,
        ),
    );
    assert_eq!(color(&editor.doc), [30, 40, 50, 255]);
    assert_eq!(set_color(&mut editor, [200, 100, 50, 255]).unwrap(), fill);
    let colored = editor.doc.clone();
    let image = replace_image(&mut editor, Arc::new(Raster::transparent(20, 40))).unwrap();
    let frame = parts(&editor.doc).unwrap().image.unwrap();
    assert_eq!(parts(&editor.doc).unwrap().fill, fill);
    assert_eq!(
        editor
            .doc
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Fill { .. }))
            .count(),
        1
    );
    // The native opaque clip boundary has no visible paint of its own.
    assert_eq!(
        flatten(&editor.doc.composite_tree(), 0).get(2, 2),
        flatten(&colored.composite_tree(), 0).get(2, 2)
    );
    assert!(is_background_node(&editor.doc, image));
    assert!(is_background_node(&editor.doc, frame.boundary));
    assert!(!is_background_node(&editor.doc, foreground));
    let before = editor.doc.clone();
    let count = editor.history.len();
    remove_image(&mut editor).unwrap();
    assert_eq!(editor.history.len(), count + 1);
    assert_eq!(editor.doc.node(foreground), colored.node(foreground));
    assert_eq!(editor.doc.node(fill), colored.node(fill));
    assert_eq!(color(&editor.doc), [200, 100, 50, 255]);
    assert!(editor.doc.node(image).is_none());
    assert!(editor.doc.node(frame.boundary).is_none());
    assert!(editor.doc.node(frame.group).is_none());
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
}

#[test]
fn replacement_and_crop_reuse_native_frame_and_preserve_sources() {
    let (mut editor, image, pixels, foreground) = fixture();
    set_image(&mut editor, image).unwrap();
    let background = parts(&editor.doc).unwrap();
    let geometry = editor
        .doc
        .node(background.image.unwrap().boundary)
        .unwrap()
        .clone();
    let before = editor.doc.clone();
    let history = editor.history.len();
    let replacement = Arc::new(Raster::solid(30, 150, [0.4, 0.2, 0.3, 0.5]));
    assert_eq!(
        replace_image(&mut editor, replacement.clone()).unwrap(),
        image
    );
    assert_eq!(editor.history.len(), history + 1);
    assert_eq!(parts(&editor.doc), Some(background));
    assert_eq!(editor.doc.node(geometry.id), Some(&geometry));
    assert_eq!(editor.doc.node(foreground), before.node(foreground));
    assert_covered(&editor.doc, image);
    let NodeKind::Raster { raster, placement } = &editor.doc.node(image).unwrap().kind else {
        panic!()
    };
    assert!(Arc::ptr_eq(raster, &replacement));
    assert_eq!(placement.rotation, 31.);
    assert!(placement.flip_x && placement.flip_y);
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    let crop = crop_frame_image(&editor.doc, image, [12., -8.], 1.7).unwrap();
    editor.execute(crop).unwrap();
    assert_covered(&editor.doc, image);
    let NodeKind::Raster { raster, .. } = &editor.doc.node(image).unwrap().kind else {
        panic!()
    };
    assert!(Arc::ptr_eq(raster, &pixels));
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
}

#[test]
fn setting_a_second_source_only_replaces_the_owned_frame() {
    let (mut editor, image, _, foreground) = fixture();
    set_image(&mut editor, image).unwrap();
    let old = parts(&editor.doc).unwrap();
    let next_pixels = Arc::new(Raster::solid(10, 30, [0.8, 0.5, 0.2, 1.]));
    let second = add(
        &mut editor,
        Node::raster(
            0,
            "Another source",
            next_pixels.clone(),
            Placement::default(),
        ),
    );
    let before = editor.doc.clone();
    set_image(&mut editor, second).unwrap();
    assert_eq!(parts(&editor.doc).unwrap().fill, old.fill);
    assert!(editor.doc.node(image).is_none());
    assert_eq!(editor.doc.node(foreground), before.node(foreground));
    let NodeKind::Raster { raster, .. } = &editor.doc.node(second).unwrap().kind else {
        panic!()
    };
    assert!(Arc::ptr_eq(raster, &next_pixels));
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
}

#[test]
fn background_roots_are_pinned_without_changing_other_root_order() {
    let (mut editor, image, _, foreground) = fixture();
    set_image(&mut editor, image).unwrap();
    let background = parts(&editor.doc).unwrap();
    let frame = background.image.unwrap();
    let extra = add(
        &mut editor,
        Node::raster(
            0,
            "Other",
            Arc::new(Raster::transparent(10, 10)),
            Placement::default(),
        ),
    );
    editor
        .execute(Command::MoveNode {
            id: extra,
            slot: Slot {
                parent: None,
                index: 0,
            },
        })
        .unwrap();
    assert_eq!(
        editor.doc.children(None),
        vec![background.fill, frame.group, extra, foreground]
    );
    assert_eq!(foreground_start(&editor.doc), 2);
    editor
        .execute(Command::MoveNode {
            id: frame.group,
            slot: Slot::TOP,
        })
        .unwrap();
    assert_eq!(
        editor.doc.children(None),
        vec![background.fill, frame.group, extra, foreground]
    );
    let mut ordinary = Document::new(10, 10);
    let a = apply(
        &mut ordinary,
        Command::AddNode {
            node: Box::new(Node::new(
                0,
                "Ordinary fill",
                NodeKind::Fill { rgba: [255; 4] },
            )),
            slot: Slot::TOP,
        },
    )
    .unwrap()
    .unwrap();
    let b = apply(
        &mut ordinary,
        Command::AddNode {
            node: Box::new(Node::group(0, "Ordinary group")),
            slot: Slot {
                parent: None,
                index: 0,
            },
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        ordinary.children(None),
        vec![b, a],
        "No role metadata means no pinning"
    );
}

#[test]
fn delete_or_detach_advanced_layers_drops_roles_without_deleting_other_content() {
    let (mut editor, image, _, foreground) = fixture();
    set_image(&mut editor, image).unwrap();
    let background = parts(&editor.doc).unwrap();
    let before = editor.doc.clone();
    editor.execute(Command::RemoveNode { id: image }).unwrap();
    assert!(parts(&editor.doc).unwrap().image.is_none());
    assert!(editor.doc.node(foreground).is_some());
    editor.undo();
    editor
        .execute(Command::MoveNode {
            id: image,
            slot: Slot::TOP,
        })
        .unwrap();
    assert!(parts(&editor.doc).unwrap().image.is_none());
    assert_eq!(editor.doc.node(image).unwrap().parent, None);
    editor.undo();
    assert_eq!(editor.doc, before);
    editor
        .execute(Command::RemoveNode {
            id: background.fill,
        })
        .unwrap();
    assert!(editor.doc.design.page_background.is_none());
    assert!(editor.doc.node(image).is_some());
    assert!(editor.doc.node(foreground).is_some());
}

#[test]
fn clipboard_preserves_artwork_but_never_adopts_the_source_page_role() {
    let (mut source, image, _, _) = fixture();
    set_image(&mut source, image).unwrap();
    let background = parts(&source.doc).unwrap();
    let fragment = Fragment::capture(
        &source.doc,
        &[background.fill, background.image.unwrap().group],
    )
    .unwrap();
    let mut destination = Editor::new(Document::new(80, 60), None);
    let before = destination.doc.clone();
    let copied = fragment
        .paste(&mut destination, Slot::TOP, (0., 0.))
        .unwrap();
    let role = destination.doc.design.page_background.unwrap();
    assert!(!copied.contains(&role.fill));
    assert!(role.image.is_none());
    assert_eq!(color(&destination.doc), [0; 4]);
    assert_eq!(destination.history.len(), 1);
    assert!(destination.undo());
    assert_eq!(destination.doc, before);
    // Whole-document cloning and an explicit ID remap retain the page role.
    assert_eq!(source.doc.clone().design.page_background, Some(background));
    let ids = HashMap::from([
        (background.fill, 101),
        (background.image.unwrap().group, 102),
        (background.image.unwrap().boundary, 103),
        (image, 104),
    ]);
    let remapped = source.doc.design.remap(&ids).page_background.unwrap();
    assert_eq!(remapped.fill, 101);
    assert_eq!(
        remapped.image.unwrap(),
        PageBackgroundImage {
            group: 102,
            boundary: 103,
            image: 104
        }
    );
}

#[test]
fn invalid_styled_locked_linked_and_nested_sources_fail_atomically() {
    let (editor, image, _, _) = fixture();
    for variation in 0..7 {
        let mut doc = editor.doc.clone();
        match variation {
            0 => doc.node_mut(image).unwrap().locked = true,
            1 => {
                doc.node_mut(image).unwrap().locks = LayerLocks {
                    position: true,
                    ..Default::default()
                }
            }
            2 => doc.node_mut(image).unwrap().opacity = 0.5,
            3 => {
                doc.node_mut(image).unwrap().mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
                    160,
                    40,
                    255,
                    |_, _| 255,
                )))
            }
            4 => doc.node_mut(image).unwrap().link_group = Some(image),
            5 => {
                let group = apply(
                    &mut doc,
                    Command::AddNode {
                        node: Box::new(Node::group(0, "Group")),
                        slot: Slot::TOP,
                    },
                )
                .unwrap()
                .unwrap();
                apply(
                    &mut doc,
                    Command::MoveNode {
                        id: image,
                        slot: Slot::top_of(Some(group)),
                    },
                )
                .unwrap();
            }
            _ => {
                doc.design.motion.insert(image, Default::default());
            }
        }
        let mut editor = Editor::new(doc.clone(), None);
        assert!(
            set_image(&mut editor, image).is_err(),
            "variation {variation}"
        );
        assert_eq!(editor.doc, doc);
        assert_eq!(editor.history.len(), 0);
    }
    let mut editor = Editor::new(editor.doc.clone(), None);
    editor.begin("Another edit");
    let before = editor.doc.clone();
    assert!(set_image(&mut editor, image).is_err());
    assert!(set_color(&mut editor, [255; 4]).is_err());
    assert!(replace_image(&mut editor, Arc::new(Raster::transparent(10, 10))).is_err());
    assert_eq!(editor.doc, before);
    assert!(editor.in_transaction());
    editor.cancel();
}

#[test]
fn metadata_defaults_validation_and_json_roundtrip_are_backward_compatible() {
    let defaults: crate::design_metadata::Design = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults.page_background, None);
    let (mut editor, image, _, _) = fixture();
    set_image(&mut editor, image).unwrap();
    let encoded = serde_json::to_string(&editor.doc.design).unwrap();
    let decoded: crate::design_metadata::Design = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, editor.doc.design);
    decoded.validate(&editor.doc).unwrap();
    let mut invalid = decoded;
    invalid
        .page_background
        .as_mut()
        .unwrap()
        .image
        .as_mut()
        .unwrap()
        .image = u64::MAX;
    assert!(invalid.validate(&editor.doc).is_err());
}

#[test]
fn native_clipping_keeps_photo_visible_and_alpha_reveals_page_color() {
    let mut editor = Editor::new(Document::new(8, 8), None);
    set_color(&mut editor, [255, 0, 0, 255]).unwrap();
    let pixels = Arc::new(Raster::from_fn(8, 8, [0; 4], |x, _| {
        if x < 4 { [0, 0, 65535, 65535] } else { [0; 4] }
    }));
    let image = replace_image(&mut editor, pixels.clone()).unwrap();
    let flat = flatten(&editor.doc.composite_tree(), 0);
    assert_eq!(
        flat.get(1, 4),
        [0, 0, 65535, 65535],
        "Invisible clip-base paint must not hide the photo"
    );
    assert_eq!(
        flat.get(6, 4),
        [65535, 0, 0, 65535],
        "Photo transparency reveals the chosen color"
    );
    let NodeKind::Raster { raster, .. } = &editor.doc.node(image).unwrap().kind else {
        panic!()
    };
    assert!(Arc::ptr_eq(raster, &pixels));
}

#[test]
fn branch_merge_preserves_background_role_and_remaps_colliding_native_nodes() {
    let base = Document::new(80, 60);
    let mut ours = Editor::new(base.clone(), None);
    let foreground = add(
        &mut ours,
        Node::path(
            0,
            "Foreground",
            Arc::new(vector_geometry::rectangle(10., 10., 10., 10.)),
            PathStyle::default(),
            80,
            60,
        ),
    );
    let mut theirs = Editor::new(base.clone(), None);
    set_color(&mut theirs, [30, 40, 50, 255]).unwrap();
    replace_image(
        &mut theirs,
        Arc::new(Raster::solid(30, 50, [0.2, 0.3, 0.4, 1.])),
    )
    .unwrap();
    let outcome = crate::graph::merge(&base, &ours.doc, &theirs.doc, &HashMap::new()).unwrap();
    let crate::graph::MergeOutcome::Merged(merged) = outcome else {
        panic!("Independent foreground/background changes should merge")
    };
    let background = merged.design.page_background.unwrap();
    assert_ne!(
        background.fill, foreground,
        "Colliding background ID must be remapped"
    );
    assert_eq!(merged.node(foreground), ours.doc.node(foreground));
    assert_eq!(color(&merged), [30, 40, 50, 255]);
    assert!(background.image.is_some());
    merged.validate().unwrap();
}

#[test]
fn ordinary_photo_fill_copy_and_paste_does_not_create_design_roles_or_extra_layers() {
    let mut source = Editor::new(Document::new(80, 60), None);
    let fill = add(
        &mut source,
        Node::new(
            0,
            "Photo background",
            NodeKind::Fill {
                rgba: [230, 210, 200, 255],
            },
        ),
    );
    let fragment = Fragment::capture(&source.doc, &[fill]).unwrap();
    assert!(fragment.design.page_background.is_none());
    let mut destination = Editor::new(Document::new(80, 60), None);
    let pasted = fragment
        .paste(&mut destination, Slot::TOP, (0., 0.))
        .unwrap();
    assert_eq!(destination.doc.nodes.len(), 1);
    assert_eq!(destination.doc.children(None), pasted);
    assert!(destination.doc.design.page_background.is_none());
    assert_eq!(destination.history.len(), 1);
    assert!(destination.undo());
    assert!(destination.doc.nodes.is_empty());
}
