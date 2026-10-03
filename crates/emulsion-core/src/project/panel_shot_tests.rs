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
    p.attach_layer_to_shot(1, hat, mia, Some(emulsion_scene::Bone::Head), None)
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
