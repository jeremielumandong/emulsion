use super::*;
use crate::storyboard::{Panel, Settings};
use emulsion_scene::{MannequinKind, PropKind};

const OBJ: &[u8] = b"o tri\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

fn rest(w: u32, h: u32) -> CameraState {
    CameraState {
        x: f64::from(w) / 2.,
        y: f64::from(h) / 2.,
        zoom: 1.,
        rotation: 0.,
    }
}

#[test]
fn parallax_is_identity_at_rest_and_on_the_panel_plane() {
    let size = (1920, 1080);
    for depth in [-0.5, 0., 1., 10.] {
        assert_eq!(
            parallax_transform(size, rest(1920, 1080), depth),
            Some(DAffine2::IDENTITY),
            "{depth}"
        );
    }
    // Any camera move leaves the panel plane where the 2D camera puts it.
    let moved = CameraState {
        x: 1200.,
        y: 400.,
        zoom: 1.7,
        rotation: 12.,
    };
    let m = parallax_transform(size, moved, 0.).unwrap();
    assert!(m.abs_diff_eq(DAffine2::IDENTITY, 1e-3), "{m:?}");
}

#[test]
fn far_layers_move_less_and_dollying_scales_near_layers_more() {
    let size = (1000, 500);
    let pan = CameraState {
        x: 600.,
        ..rest(1000, 500)
    };
    // The camera pans 100 px right: the panel plane slides 100 px left on
    // screen. A layer at depth 1 (twice as far) slides half as far, so it
    // sits 50 px right of where it was drawn on the panel.
    let far = parallax_transform(size, pan, 1.).unwrap();
    let centre = far.transform_point2(dvec2(500., 250.));
    assert!((centre - dvec2(550., 250.)).length() < 0.05, "{centre}");
    assert!(
        (far.matrix2.x_axis.x - 1.).abs() < 1e-4,
        "panning does not scale"
    );
    // A near layer (depth -0.5) moves the other way, twice as fast.
    let near = parallax_transform(size, pan, -0.5).unwrap();
    let centre = near.transform_point2(dvec2(500., 250.));
    assert!((centre - dvec2(400., 250.)).length() < 0.05, "{centre}");
    // Zoom 2 dollies in halfway: the plane doubles, a far layer grows less
    // and a near one more, relative to the panel the camera frames.
    let dolly = CameraState {
        zoom: 2.,
        ..rest(1000, 500)
    };
    let s = |depth: f64| {
        parallax_transform(size, dolly, depth)
            .unwrap()
            .matrix2
            .x_axis
            .x
    };
    assert!((s(1.) - 2. / 3.).abs() < 1e-3, "{}", s(1.));
    assert!(s(-0.25) > 1.4);
    // Past the camera the layer is hidden.
    assert!(parallax_transform(size, dolly, -0.6).is_none());
}

#[test]
fn parallax_panel_moves_only_layers_in_depth() {
    let mut board = Storyboard::new(Settings::new(200, 100), &[1]);
    let mut doc = Document::new(200, 100);
    let fill = |name: &str| crate::Node::new(0, name, crate::NodeKind::Fill { rgba: [255; 4] });
    for name in ["Sky", "Hero"] {
        Command::AddNode {
            node: Box::new(fill(name)),
            slot: crate::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
    }
    let ids: Vec<_> = doc.nodes.iter().map(|n| n.id).collect();
    board.set_layer_depth(1, ids[0], 4.).unwrap();
    assert!(board.set_layer_depth(1, ids[0], -2.).is_err());
    assert!(board.has_parallax(1));
    board.validate(&[1]).unwrap();
    let still = board.parallax_panel(1, &doc, board.rest_camera()).unwrap();
    assert_eq!(still, doc);
    let state = CameraState {
        x: 150.,
        ..board.rest_camera()
    };
    let moved = board.parallax_panel(1, &doc, state).unwrap();
    assert_ne!(moved.node(ids[0]), doc.node(ids[0]), "the far layer moved");
    assert_eq!(
        moved.node(ids[1]),
        doc.node(ids[1]),
        "the panel plane stays"
    );
    board.set_layer_depth(1, ids[0], 0.).unwrap();
    assert!(!board.has_parallax(1));
}

#[test]
fn panels_and_boards_without_sets_serialize_as_before() {
    let panel = Panel::new(1, 24);
    let json = serde_json::to_value(&panel).unwrap();
    assert!(json.get("shot").is_none() && json.get("depth").is_none());
    let board = Storyboard::new(Settings::new(64, 36), &[1]);
    let json = serde_json::to_value(&board).unwrap();
    assert!(json.get("shot_library").is_none());
    // An old board (without the fields) reads with empty defaults.
    let back: Storyboard = serde_json::from_value(json).unwrap();
    assert_eq!(back, board);
}

#[test]
fn sets_round_trip_and_invalid_sets_are_refused() {
    let mut board = Storyboard::new(Settings::new(64, 36), &[1]);
    let mut shot = PanelShot::new(board.aspect());
    assert!(
        (shot.set.camera.film_back.width_mm / shot.set.camera.film_back.height_mm - 64. / 36.)
            .abs()
            < 1e-3
    );
    shot.set.add_character(
        "Mia",
        emulsion_scene::Character::of(MannequinKind::AdultFemale),
        Vec3::ZERO,
        0.,
    );
    shot.set
        .add_prop("Chair", Prop::builtin(PropKind::Chair), Vec3::X, 0.);
    board.panels.get_mut(&1).unwrap().shot = Some(Box::new(shot.clone()));
    board.validate(&[1]).unwrap();
    let json = serde_json::to_string(&board).unwrap();
    let back: Storyboard = serde_json::from_str(&json).unwrap();
    assert_eq!(back.panels[&1].shot.as_deref(), Some(&shot));
    // Out-of-range values do not validate.
    let mut bad = shot.clone();
    bad.reference.opacity = 0.;
    board.panels.get_mut(&1).unwrap().shot = Some(Box::new(bad));
    assert!(board.validate(&[1]).is_err());
    let mut bad = shot;
    bad.set.camera.focal_length_mm = f32::NAN;
    board.panels.get_mut(&1).unwrap().shot = Some(Box::new(bad));
    assert!(board.validate(&[1]).is_err());
}

#[test]
fn the_library_keeps_one_copy_of_each_model_and_named_poses() {
    let mut library = ShotLibrary::default();
    let id = library.add_model("Crate.obj", OBJ.to_vec()).unwrap();
    assert_eq!(library.add_model("again.obj", OBJ.to_vec()).unwrap(), id);
    assert_eq!(library.models.len(), 1);
    assert_eq!(library.models[&id].name, "Crate");
    assert_eq!(library.entry_name(&id).unwrap(), format!("models/{id}.obj"));
    assert!(library.assets().get(&id).is_some());
    library.check_models().unwrap();
    library.validate().unwrap();
    // A .gltf whose buffers sit in other files is refused with a way out.
    let gltf = br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"mesh.bin","byteLength":12}],"bufferViews":[{"buffer":0,"byteLength":12}],"accessors":[{"bufferView":0,"componentType":5126,"count":1,"type":"VEC3"}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}]}"#;
    let error = library.add_model("scene.gltf", gltf.to_vec()).unwrap_err();
    assert!(error.contains(".glb"), "{error}");
    assert!(library.add_model("notes.txt", b"hello".to_vec()).is_err());
    // Poses replace by name.
    let walk = emulsion_scene::PosePreset::Walk.pose();
    library.save_pose("Hero walk", &walk).unwrap();
    library
        .save_pose("Hero walk", &emulsion_scene::Pose::rest())
        .unwrap();
    assert_eq!(library.poses.len(), 1);
    assert_eq!(library.poses[0].name, "Hero walk");
    assert!(library.save_pose("  ", &walk).is_err());
}
