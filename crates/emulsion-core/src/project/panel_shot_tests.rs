use super::*;
use crate::project::ProjectKind;
use crate::storyboard::Panel;
use crate::storyboard_shot::LayerAttachment;
use emulsion_scene::{Character, MannequinKind, PosePreset, RenderStyle};

const OBJ: &[u8] = b"o tri\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

fn board() -> ProjectEditor {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    p.insert_panels(
        Some(1),
        &blank,
        vec![("Panel 2".into(), Panel::new(0, 24))],
        None,
    )
    .unwrap();
    p
}

fn add_mia(p: &mut ProjectEditor, panel: PageId) -> ObjectId {
    let mut id = None;
    p.edit_panel_shot(panel, "Add character", |shot, _| {
        id = Some(shot.set.add_character(
            "Mia",
            Character::of(MannequinKind::AdultFemale),
            Vec3::ZERO,
            0.,
        ));
        Ok(())
    })
    .unwrap();
    id.unwrap()
}

fn layers(p: &ProjectEditor, panel: PageId) -> Vec<String> {
    p.page(panel)
        .unwrap()
        .doc
        .nodes
        .iter()
        .map(|n| n.name.clone())
        .collect()
}

#[test]
fn each_set_edit_is_one_undo_step() {
    let mut p = board();
    assert!(p.panel_shot(1).is_none());
    let mia = add_mia(&mut p, 1);
    assert_eq!(p.panel_shot(1).unwrap().set.character_ids(), [mia]);
    p.edit_panel_shot(1, "Pose", |shot, _| {
        shot.set.character_mut(mia).unwrap().pose = PosePreset::Wave.pose();
        shot.set.translate(mia, Vec3::X);
        Ok(())
    })
    .unwrap();
    assert!(p.undo());
    let set = &p.panel_shot(1).unwrap().set;
    assert_eq!(
        set.object(mia).unwrap().transform.position,
        Vec3::ZERO,
        "one step undoes both"
    );
    assert!(p.undo());
    assert!(p.panel_shot(1).is_none(), "the set itself was one step");
    assert!(p.redo());
    assert!(p.panel_shot(1).is_some());
    // Invalid edits change nothing.
    let error = p
        .edit_panel_shot(1, "Bad", |shot, _| {
            shot.reference.opacity = 3.;
            Ok(())
        })
        .unwrap_err();
    assert!(error.contains("opacity"), "{error}");
    // A locked panel refuses set edits.
    p.edit_storyboard(|b| {
        b.panels.get_mut(&1).unwrap().locked = true;
        Ok(())
    })
    .unwrap();
    assert!(
        p.edit_panel_shot(1, "Locked", |s, _| {
            s.set.camera.roll = 5.;
            Ok(())
        })
        .is_err()
    );
}

#[test]
fn the_reference_layer_renders_at_panel_size_and_is_replaced() {
    let mut p = board();
    add_mia(&mut p, 1);
    let before = layers(&p, 1);
    let (image, set) = p.render_panel_shot(1).unwrap();
    assert_eq!((image.width, image.height), (64, 36));
    let id = p.set_shot_reference(1, &image, &set, false).unwrap();
    let doc = &p.page(1).unwrap().doc;
    let node = doc.node(id).unwrap();
    assert_eq!(node.name, REFERENCE_LAYER);
    assert!(node.locked && (node.opacity - 0.5).abs() < 1e-6);
    let NodeKind::Raster { raster, .. } = &node.kind else {
        panic!("a raster layer");
    };
    assert_eq!((raster.width(), raster.height()), (64, 36));
    // Just above the paper.
    let roots = doc.children(None);
    let paper = matches!(doc.node(roots[0]).unwrap().kind, NodeKind::Fill { .. });
    assert_eq!(
        roots.iter().position(|r| *r == id),
        Some(usize::from(paper))
    );
    assert_eq!(p.panel_shot(1).unwrap().layer, Some(id));
    // Rendering again replaces the pixels in the same layer.
    p.edit_panel_shot(1, "Style", |shot, _| {
        shot.reference.style = RenderStyle::Outline;
        shot.reference.opacity = 0.3;
        Ok(())
    })
    .unwrap();
    let again = p.update_shot_reference(1).unwrap();
    assert_eq!(again, id);
    assert_eq!(layers(&p, 1).len(), before.len() + 1);
    assert!((p.page(1).unwrap().doc.node(id).unwrap().opacity - 0.3).abs() < 1e-6);
    // The render joined the style edit: one Undo takes both back.
    assert!(p.undo());
    assert_eq!(p.panel_shot(1).unwrap().reference.style, RenderStyle::Toon);
    assert!((p.page(1).unwrap().doc.node(id).unwrap().opacity - 0.5).abs() < 1e-6);
    // A render of a set that has changed since is refused.
    let (image, stale) = p.render_panel_shot(1).unwrap();
    add_mia(&mut p, 1);
    assert!(p.set_shot_reference(1, &image, &stale, true).is_err());
    // Panels without a set have no reference.
    assert!(p.render_panel_shot(2).is_err());
}

#[test]
fn set_edits_report_a_stale_reference_and_join_its_render() {
    let mut p = board();
    let mia = add_mia(&mut p, 1);
    assert!(
        !p.edit_panel_shot(1, "Move", |s, _| {
            s.set.translate(mia, Vec3::X);
            Ok(())
        })
        .unwrap(),
        "no reference layer yet"
    );
    p.update_shot_reference(1).unwrap();
    let stale = p
        .edit_panel_shot(1, "Move", |s, _| {
            s.set.translate(mia, Vec3::Z);
            Ok(())
        })
        .unwrap();
    assert!(stale);
    let pixels = |p: &ProjectEditor| p.page(1).unwrap().doc.clone();
    let moved_before = pixels(&p);
    p.update_shot_reference(1).unwrap();
    assert_ne!(pixels(&p), moved_before);
    assert!(p.undo(), "one Undo for the move and its new reference");
    assert_eq!(
        p.panel_shot(1)
            .unwrap()
            .set
            .object(mia)
            .unwrap()
            .transform
            .position,
        Vec3::X
    );
    assert_eq!(pixels(&p).nodes.len(), moved_before.nodes.len());
}

#[test]
fn describing_a_shot_builds_the_panels_set() {
    let mut p = board();
    let described = p
        .describe_panel_shot(2, "low-angle close-up of two people at a table, 85mm")
        .unwrap();
    assert!(!described.interpretation.is_empty());
    let set = &p.panel_shot(2).unwrap().set;
    assert_eq!(set.character_ids().len(), 2);
    assert!((set.camera.focal_length_mm - 85.).abs() < 0.5);
    assert!(p.panel_shot(1).is_none(), "other panels are unaffected");
    assert!(p.undo());
    assert!(p.panel_shot(2).is_none());
}

#[test]
fn snapshots_are_editable_layers_and_models_travel_in_the_library() {
    let mut p = board();
    let object = p
        .import_shot_model(1, "Crate.obj", OBJ.to_vec(), Vec3::ZERO)
        .unwrap();
    let board = p.storyboard().unwrap();
    assert_eq!(board.shot_library.models.len(), 1);
    let (image, _) = p.render_panel_shot(1).unwrap();
    let id = p.snapshot_shot(1, &image).unwrap();
    let node = p.page(1).unwrap().doc.node(id).unwrap();
    assert_eq!(node.name, SNAPSHOT_LAYER);
    assert!(!node.locked && node.opacity == 1.);
    // Removing the model's last prop drops the model; Undo restores it.
    p.edit_panel_shot(1, "Delete", |s, _| {
        s.set.remove(object);
        Ok(())
    })
    .unwrap();
    assert!(p.storyboard().unwrap().shot_library.models.is_empty());
    assert!(p.undo());
    assert_eq!(p.storyboard().unwrap().shot_library.models.len(), 1);
    // Bad files change nothing.
    assert!(
        p.import_shot_model(1, "x.obj", b"nothing".to_vec(), Vec3::ZERO)
            .is_err()
    );
}

#[test]
fn attached_layers_follow_the_set_in_the_same_step() {
    let mut p = board();
    let mia = add_mia(&mut p, 1);
    let mut doc = p.page(1).unwrap().doc.clone();
    Command::AddNode {
        node: Box::new(Node::new(0, "Hat", NodeKind::Fill { rgba: [255; 4] })),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    let hat = doc.nodes.iter().map(|n| n.id).max().unwrap();
    p.commit_documents(BTreeMap::from([(1, doc)]), "Hat")
        .unwrap();
    p.attach_layer_to_shot(1, hat, mia, Some(emulsion_scene::Bone::Head), None, None)
        .unwrap();
    let a: LayerAttachment = p.panel_shot(1).unwrap().attachments[&hat];
    let before = p.page(1).unwrap().doc.node(hat).cloned();
    p.edit_panel_shot(1, "Move Mia", |s, _| {
        s.set.translate(mia, Vec3::new(0.5, 0., 0.));
        Ok(())
    })
    .unwrap();
    let after = p.panel_shot(1).unwrap().attachments[&hat];
    assert!(
        (after.screen[0] - a.screen[0]).abs() > 1.,
        "the point moved on screen"
    );
    assert_ne!(
        p.page(1).unwrap().doc.node(hat).cloned(),
        before,
        "and the layer with it"
    );
    assert!(p.undo());
    assert_eq!(
        p.page(1).unwrap().doc.node(hat).cloned(),
        before,
        "one step"
    );
    assert_eq!(p.panel_shot(1).unwrap().attachments[&hat], a);
    // Removing the character lets the layer go.
    p.edit_panel_shot(1, "Delete", |s, _| {
        s.set.remove(mia);
        Ok(())
    })
    .unwrap();
    assert!(p.panel_shot(1).unwrap().attachments.is_empty());
}

/// A board `w × h` with a 4 m wall facing the camera on panel 1 and a red
/// 40 × 20 layer "Sign" over its middle.
fn walled(w: u32, h: u32) -> (ProjectEditor, ObjectId, NodeId) {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(w, h)).unwrap();
    let mut wall = None;
    p.edit_panel_shot(1, "Wall", |shot, _| {
        wall = Some(shot.set.add_prop(
            "Wall",
            emulsion_scene::Prop::builtin(emulsion_scene::PropKind::Wall),
            Vec3::ZERO,
            0.,
        ));
        Ok(())
    })
    .unwrap();
    let mut doc = p.page(1).unwrap().doc.clone();
    let sign = Node::raster(
        0,
        "Sign",
        Arc::new(emulsion_raster::Raster::solid(40, 20, [1., 0., 0., 1.])),
        emulsion_raster::Placement {
            x: f64::from(w / 2 - 20),
            y: f64::from(h / 2 - 10),
            ..Default::default()
        },
    );
    Command::AddNode {
        node: Box::new(sign),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    let id = doc.nodes.iter().map(|n| n.id).max().unwrap();
    p.commit_documents(BTreeMap::from([(1, doc)]), "Sign")
        .unwrap();
    (p, wall.unwrap(), id)
}

fn raster_of(
    p: &ProjectEditor,
    id: NodeId,
) -> (Arc<emulsion_raster::Raster>, emulsion_raster::Placement) {
    match &p.page(1).unwrap().doc.node(id).unwrap().kind {
        NodeKind::Raster { raster, placement } => (raster.clone(), *placement),
        _ => panic!("a raster layer"),
    }
}

/// Opaque columns of a layer, and the opaque height of its first and last
/// such column (in panel pixels).
fn coverage(raster: &emulsion_raster::Raster) -> (u32, u32, u32) {
    let opaque = |x: u32| {
        (0..raster.height())
            .filter(|y| raster.get(x, *y)[3] > 40000)
            .count() as u32
    };
    let columns: Vec<u32> = (0..raster.width()).filter(|x| opaque(*x) > 0).collect();
    let (first, last) = (columns[0], *columns.last().unwrap());
    (columns.len() as u32, opaque(first), opaque(last))
}

#[test]
fn layers_on_a_surface_take_its_angle_and_follow_it() {
    let (mut p, wall, sign) = walled(320, 180);
    let flat = raster_of(&p, sign);
    // Pick the wall under the sign's centre and lay the sign on it.
    let hit = p.pick_panel_shot(1, 160., 90.).unwrap();
    assert_eq!(hit.object, wall);
    assert!(
        hit.normal.z > 0.9,
        "the wall faces the camera: {}",
        hit.normal
    );
    p.attach_layer_to_shot(1, sign, wall, None, Some(hit.point), Some(hit.normal))
        .unwrap();
    let a = p.panel_shot(1).unwrap().attachments[&sign];
    let surface = a.surface.expect("laid on the surface");
    // The flat drawing is kept in a hidden, locked copy just below.
    let doc = &p.page(1).unwrap().doc;
    let copy = doc.node(surface.flat).unwrap();
    assert_eq!(copy.name, "Sign (flat)");
    assert!(!copy.visible && copy.locked);
    let NodeKind::Raster { raster, .. } = &copy.kind else {
        panic!("a raster copy")
    };
    assert!(Arc::ptr_eq(raster, &flat.0));
    let roots = doc.children(None);
    let at = |id| roots.iter().position(|r| *r == id).unwrap();
    assert_eq!(at(surface.flat) + 1, at(sign));
    // Seen square on, the sign keeps its size where it was drawn.
    let (laid, placement) = raster_of(&p, sign);
    let (width, left, right) = coverage(&laid);
    assert!((38..=42).contains(&width), "{width}");
    assert!(
        left.abs_diff(right) <= 1 && (18..=22).contains(&left),
        "{left} {right}"
    );
    assert!((placement.x - 140.).abs() <= 2. && (placement.y - 80.).abs() <= 2.);
    // Turning the wall 60° foreshortens it: narrower, and taller on the
    // side that comes nearer the camera.
    p.edit_panel_shot(1, "Turn wall", |s, _| {
        s.set.set_rotation_euler(wall, 60., 0., 0.);
        Ok(())
    })
    .unwrap();
    let (turned, _) = raster_of(&p, sign);
    let (width, left, right) = coverage(&turned);
    assert!(width < 28 && width > 10, "{width}");
    assert!(
        left.abs_diff(right) >= 1,
        "a perspective warp: {left} vs {right}"
    );
    // The anchor stays on the wall's point, which turned with it.
    let b = p.panel_shot(1).unwrap().attachments[&sign];
    let shot = p.panel_shot(1).unwrap();
    let turned_point = glam::Quat::from_rotation_y(60f32.to_radians()) * hit.point;
    let seen = emulsion_scene::project_point(&shot.set.camera, 320, 180, turned_point).unwrap();
    assert!(
        (b.screen[0] - f64::from(seen.x)).abs() < 0.01
            && (b.screen[1] - f64::from(seen.y)).abs() < 0.01,
        "{:?} vs {seen:?}",
        b.screen
    );
    // The warp always starts from the flat drawing: turning back restores
    // the first warp exactly.
    p.edit_panel_shot(1, "Turn back", |s, _| {
        s.set.set_rotation_euler(wall, 0., 0., 0.);
        Ok(())
    })
    .unwrap();
    assert_eq!(raster_of(&p, sign).0.to_pixels(), laid.to_pixels());
    // Each turn was one Undo step, layer and set together.
    assert!(p.undo());
    assert!(Arc::ptr_eq(&raster_of(&p, sign).0, &turned));
    assert!(p.undo());
    assert!(Arc::ptr_eq(&raster_of(&p, sign).0, &laid));
    p.redo();
    p.redo();
    // Moving the wall moves the layer with it; Undo takes it back.
    p.edit_panel_shot(1, "Move wall", |s, _| {
        s.set.translate(wall, Vec3::new(0.5, 0., 0.));
        Ok(())
    })
    .unwrap();
    assert!(raster_of(&p, sign).1.x > placement.x + 5.);
    assert!(p.undo());
    assert_eq!(raster_of(&p, sign).1, placement);
    // Point-follow stays available, and other layer kinds are refused.
    p.attach_layer_to_shot(1, sign, wall, None, Some(hit.point), None)
        .unwrap();
    assert!(
        p.panel_shot(1).unwrap().attachments[&sign]
            .surface
            .is_none()
    );
}

#[test]
fn surfaces_need_a_pixel_layer() {
    let (mut p, wall, _) = walled(160, 90);
    let mut doc = p.page(1).unwrap().doc.clone();
    Command::AddNode {
        node: Box::new(Node::new(0, "Fill", NodeKind::Fill { rgba: [9; 4] })),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    let fill = doc.nodes.iter().map(|n| n.id).max().unwrap();
    p.commit_documents(BTreeMap::from([(1, doc)]), "Fill")
        .unwrap();
    let hit = p.pick_panel_shot(1, 80., 45.).unwrap();
    let error = p
        .attach_layer_to_shot(1, fill, wall, None, Some(hit.point), Some(hit.normal))
        .unwrap_err();
    assert!(error.contains("Rasterize"), "{error}");
}

#[test]
fn models_travel_with_copied_panels_and_library_items() {
    let mut a = board();
    let mut doc = a.page(1).unwrap().doc.clone();
    Command::AddNode {
        node: Box::new(Node::new(0, "Sky", NodeKind::Fill { rgba: [9; 4] })),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    a.commit_documents(BTreeMap::from([(1, doc)]), "Sky")
        .unwrap();
    let crate_id = a
        .import_shot_model(1, "Crate.obj", OBJ.to_vec(), Vec3::ZERO)
        .unwrap();
    let model = a
        .storyboard()
        .unwrap()
        .shot_library
        .models
        .keys()
        .next()
        .unwrap()
        .clone();
    let clip = a.copy_panels(&[1]).unwrap();
    assert_eq!(clip.models.models.len(), 1);
    // Pasted into another project, the set keeps its model; pasting again
    // keeps one copy.
    let mut b = board();
    b.paste_panels(Some(1), &clip).unwrap();
    b.paste_panels(Some(1), &clip).unwrap();
    let library = &b.storyboard().unwrap().shot_library;
    assert_eq!(library.models.keys().collect::<Vec<_>>(), [&model]);
    assert_eq!(
        library.models[&model].data,
        a.storyboard().unwrap().shot_library.models[&model].data
    );
    assert!(b.shot_assets().get(&model).is_some());
    // A project whose model budget is full refuses with a clear message.
    let mut full = board();
    full.edit_storyboard(|board| {
        for i in 0..crate::storyboard_shot::MAX_MODELS {
            let obj = format!("o m{i}\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
            board
                .shot_library
                .add_model(&format!("m{i}.obj"), obj.into_bytes())?;
        }
        Ok(())
    })
    .unwrap();
    let error = full.paste_panels(Some(1), &clip).unwrap_err();
    assert!(error.contains("do not fit"), "{error}");
    assert_eq!(full.page_list().len(), 2, "nothing pasted");
    // A panel item with a set carries its model to another project.
    let item = a.capture_panel_item(1).unwrap();
    let animation = item.animation.as_ref().expect("the set is kept");
    assert!(animation.panels[0].panel.shot.is_some());
    assert_eq!(animation.models.models.len(), 1);
    let mut c = board();
    c.place_item(&item).unwrap();
    assert!(
        c.storyboard()
            .unwrap()
            .shot_library
            .models
            .contains_key(&model)
    );
    // In the project library the project keeps the model, even once no
    // panel uses it.
    let saved = a.add_library_panel(1, "Crate shot", &[]).unwrap();
    let stored = a.storyboard().unwrap().library.item(saved).unwrap().clone();
    assert!(stored.animation.as_ref().unwrap().models.is_empty());
    a.edit_panel_shot(1, "Delete", |s, _| {
        s.set.remove(crate_id);
        Ok(())
    })
    .unwrap();
    assert!(
        a.storyboard()
            .unwrap()
            .shot_library
            .models
            .contains_key(&model)
    );
    // Extracting a scene keeps only the models its sets use.
    let project = a.snapshot().unwrap();
    let scene = project.storyboard.as_ref().unwrap().panels[&2].scene;
    let extract =
        crate::storyboard_extract::extract_scenes(&project, &[scene], "Board", 0).unwrap();
    let kept = &extract.storyboard.unwrap().shot_library;
    assert_eq!(kept.models.len(), 1, "the library item's model stays");
}
